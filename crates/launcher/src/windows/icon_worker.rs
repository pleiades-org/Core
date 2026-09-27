use super::{application_icon::ApplicationIcon, favicon::WebsiteOrigin};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

pub const ICONS_READY: u32 = WM_APP + 5;
const CACHE_CAPACITY: usize = 64;
/// A just-installed app or a waking drive can fail once; retry instead of a permanent blank.
const FAILED_ICON_RETRY: Duration = Duration::from_secs(30);

pub struct IconRequest {
    pub identifier: Arc<str>,
    pub source: IconSource,
}

/// Where a result's icon comes from.
pub enum IconSource {
    /// An application, file or folder: the Windows Shell icon.
    Shell(PathBuf),
    /// A website quicklink: the site's cached or downloaded favicon.
    Website(WebsiteOrigin),
}

impl IconSource {
    fn load(&self, allow_network: bool) -> Option<ApplicationIcon> {
        match self {
            Self::Shell(path) => {
                let icon = ApplicationIcon::load(path);
                if icon.is_none() {
                    eprintln!("Application icon unavailable: {}", path.display());
                }
                icon
            }
            // Dry runs and probes never contact websites.
            Self::Website(origin) => allow_network.then(|| super::favicon::load(origin))?,
        }
    }
}

#[derive(Clone)]
pub struct LoadedIcon {
    pub identifier: Arc<str>,
    pub icon: Option<Arc<ApplicationIcon>>,
    loaded_at: Instant,
}

struct RequestBatch {
    generation: u64,
    requests: Vec<IconRequest>,
}

#[derive(Default)]
struct Pending {
    batch: Option<RequestBatch>,
    shutdown: bool,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
    changed: Condvar,
    generation: AtomicU64,
    completed: Mutex<Option<Vec<LoadedIcon>>>,
}

pub struct IconWorker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl IconWorker {
    pub fn new(window: HWND, allow_network: bool) -> std::io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let worker_shared = shared.clone();
        let address = window.0 as usize;
        let thread = thread::Builder::new()
            .name("core-icons".into())
            .spawn(move || run(address, worker_shared, allow_network))?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    pub fn submit(&self, requests: Vec<IconRequest>) {
        let generation = self.shared.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let should_wake = !requests.is_empty();
        let batch = should_wake.then_some(RequestBatch {
            generation,
            requests,
        });
        self.shared.pending.lock().expect("icon request lock").batch = batch;
        if should_wake {
            self.shared.changed.notify_one();
        }
    }

    pub fn take_completed(&self) -> Option<Vec<LoadedIcon>> {
        self.shared
            .completed
            .lock()
            .expect("icon result lock")
            .take()
    }
}

impl Drop for IconWorker {
    fn drop(&mut self) {
        {
            let mut pending = self.shared.pending.lock().expect("icon request lock");
            pending.shutdown = true;
            pending.batch = None;
            self.shared.generation.fetch_add(1, Ordering::AcqRel);
        }
        self.shared.changed.notify_one();
        // A third-party Shell extension may stall. Never block the UI on that call at exit.
        // The shared shutdown flag prevents later posts; thread-local icons drop on return.
        if let Some(thread) = self.thread.take().filter(JoinHandle::is_finished) {
            if thread.join().is_err() {
                eprintln!("Core icon worker stopped unexpectedly");
            }
        }
    }
}

fn run(address: usize, shared: Arc<Shared>, allow_network: bool) {
    let mut cache = VecDeque::with_capacity(CACHE_CAPACITY);
    // Initialize lazily so a hidden launcher does not load the Shell icon machinery.
    let mut com = None;
    while let Some(batch) = next_batch(&shared) {
        if com.is_none() {
            match ComApartment::new() {
                Ok(apartment) => com = Some(apartment),
                Err(error) => {
                    eprintln!("Application icons unavailable: {error}");
                    return;
                }
            }
        }
        // Local icons first: a slow website must not delay application icons.
        let (websites, local): (Vec<_>, Vec<_>) = batch
            .requests
            .into_iter()
            .partition(|request| matches!(request.source, IconSource::Website(_)));
        let current = || shared.generation.load(Ordering::Acquire) == batch.generation;
        let mut loaded = Vec::with_capacity(local.len());
        for request in local {
            if !current() {
                break;
            }
            loaded.push(cached_icon(&mut cache, request, allow_network));
        }
        publish(address, &shared, loaded);
        // Downloads are slow and re-renders (search results, exchange rates, discovery) start
        // new batches often, so each website icon is published the moment it is ready.
        for request in websites {
            if !current() {
                break;
            }
            publish(
                address,
                &shared,
                vec![cached_icon(&mut cache, request, allow_network)],
            );
        }
    }
    // Release icons before the COM apartment is uninitialized.
    drop(cache);
}

fn next_batch(shared: &Shared) -> Option<RequestBatch> {
    let mut pending = shared.pending.lock().expect("icon request lock");
    while pending.batch.is_none() && !pending.shutdown {
        pending = shared.changed.wait(pending).expect("icon wake lock");
    }
    if pending.shutdown {
        None
    } else {
        pending.batch.take()
    }
}

fn cached_icon(
    cache: &mut VecDeque<LoadedIcon>,
    request: IconRequest,
    allow_network: bool,
) -> LoadedIcon {
    let cached = cache
        .iter()
        .position(|entry| entry.identifier == request.identifier)
        .map(|index| cache.remove(index).expect("cached icon index"))
        .filter(|entry| entry.icon.is_some() || entry.loaded_at.elapsed() < FAILED_ICON_RETRY);
    let loaded = if let Some(entry) = cached {
        entry
    } else {
        let icon = request.source.load(allow_network).map(Arc::new);
        LoadedIcon {
            identifier: request.identifier,
            icon,
            loaded_at: Instant::now(),
        }
    };
    if cache.len() == CACHE_CAPACITY {
        cache.pop_front();
    }
    cache.push_back(loaded.clone());
    loaded
}

/// Finished icons are always delivered, even when a newer batch exists: the view matches them
/// to rows by identifier and ignores any that are no longer shown.
fn publish(address: usize, shared: &Shared, loaded: Vec<LoadedIcon>) {
    if loaded.is_empty() {
        return;
    }
    // Synchronize posting with shutdown so a detached loader cannot post to a reused HWND.
    let pending = shared.pending.lock().expect("icon request lock");
    if pending.shutdown {
        return;
    }
    // Several publishes may arrive before the UI takes them; keep them all.
    let mut completed = shared.completed.lock().expect("icon result lock");
    let should_wake = completed.is_none();
    completed.get_or_insert_with(Vec::new).extend(loaded);
    drop(completed);
    if should_wake {
        if let Err(error) = unsafe {
            PostMessageW(
                Some(HWND(address as *mut _)),
                ICONS_READY,
                WPARAM(0),
                LPARAM(0),
            )
        } {
            eprintln!("Could not notify Core about application icons: {error}");
        }
    }
}

struct ComApartment;
impl ComApartment {
    fn new() -> windows::core::Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetGuiResources, GR_USEROBJECTS};

    #[test]
    fn finished_icons_survive_newer_batches_and_accumulate() {
        // Publishing wakes a null window: the notification lands on this thread's queue.
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let shared = Shared::default();
        let loaded = |identifier: &str| LoadedIcon {
            identifier: identifier.into(),
            icon: None,
            loaded_at: Instant::now(),
        };
        publish(0, &shared, vec![loaded("github")]);
        // A re-render started a newer batch while the next website icon was downloading.
        shared.generation.fetch_add(1, Ordering::AcqRel);
        publish(0, &shared, vec![loaded("youtube")]);
        publish(0, &shared, Vec::new());
        let completed = shared.completed.lock().unwrap().take().unwrap();
        let identifiers: Vec<&str> = completed.iter().map(|icon| &*icon.identifier).collect();
        assert_eq!(identifiers, ["github", "youtube"]);
    }

    #[test]
    fn failed_icons_are_cached_briefly_then_retried() {
        let _apartment = ComApartment::new().unwrap();
        let request = || IconRequest {
            identifier: "missing".into(),
            source: IconSource::Shell(PathBuf::from(r"C:\Core\definitely-missing\app.exe")),
        };
        let mut cache = VecDeque::new();
        let first = cached_icon(&mut cache, request(), false);
        assert!(first.icon.is_none());
        assert_eq!(
            cached_icon(&mut cache, request(), false).loaded_at,
            first.loaded_at
        );
        cache[0].loaded_at = Instant::now() - FAILED_ICON_RETRY;
        let retried = cached_icon(&mut cache, request(), false);
        assert!(retried.loaded_at > first.loaded_at);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn icon_cache_reuses_handles_stays_bounded_and_releases_evictions() {
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _apartment = ComApartment::new().unwrap();
        let path = std::env::current_exe().unwrap();
        // Warm up Shell initialization before counting the icon handles that we own.
        drop(ApplicationIcon::load(&path).expect("executable Shell icon"));
        let handles = || unsafe { GetGuiResources(GetCurrentProcess(), GR_USEROBJECTS) };
        let baseline = handles();
        let mut cache = VecDeque::new();
        let first = cached_icon(
            &mut cache,
            IconRequest {
                identifier: "first".into(),
                source: IconSource::Shell(path.clone()),
            },
            false,
        );
        let repeated = cached_icon(
            &mut cache,
            IconRequest {
                identifier: "first".into(),
                source: IconSource::Shell(path.clone()),
            },
            false,
        );
        assert!(Arc::ptr_eq(
            first.icon.as_ref().unwrap(),
            repeated.icon.as_ref().unwrap()
        ));
        drop(repeated);
        let retained = Arc::downgrade(first.icon.as_ref().unwrap());
        for index in 0..CACHE_CAPACITY * 3 {
            let loaded = cached_icon(
                &mut cache,
                IconRequest {
                    identifier: format!("app-{index}").into(),
                    source: IconSource::Shell(path.clone()),
                },
                false,
            );
            assert!(loaded.icon.is_some());
            assert!(cache.len() <= CACHE_CAPACITY);
        }
        assert!(handles() <= baseline + CACHE_CAPACITY as u32 + 1);
        assert!(
            retained.upgrade().is_some(),
            "visible row must survive eviction"
        );
        drop(first);
        assert!(
            retained.upgrade().is_none(),
            "evicted icon must release its last owner"
        );
        drop(cache);
        assert_eq!(
            handles(),
            baseline,
            "icon handles leaked after cache release"
        );
    }
}
