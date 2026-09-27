use super::{application_icon::ApplicationIcon, favicon::WebsiteOrigin};
use std::{
    collections::{HashMap, VecDeque},
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

impl LoadedIcon {
    /// An icon as a worker delivers it, for view tests.
    #[cfg(test)]
    pub fn new(identifier: Arc<str>, icon: Option<Arc<ApplicationIcon>>) -> Self {
        Self {
            identifier,
            icon,
            loaded_at: Instant::now(),
        }
    }

    /// Until when a failed icon stays failed: the worker loads it again only after this.
    fn missing_until(&self) -> Option<Instant> {
        self.icon
            .is_none()
            .then(|| self.loaded_at + FAILED_ICON_RETRY)
    }
}

/// Icons that failed recently, kept on the UI thread. Renders skip asking for them until the
/// worker would retry, so a missing icon does not wake the worker and repaint the list for
/// nothing on every keystroke.
#[derive(Default)]
pub struct MissingIcons(HashMap<Arc<str>, Instant>);

impl MissingIcons {
    pub fn record(&mut self, loaded: &[LoadedIcon], now: Instant) {
        self.0.retain(|_, until| *until > now);
        for icon in loaded {
            match icon.missing_until().filter(|until| *until > now) {
                Some(until) => {
                    self.0.insert(icon.identifier.clone(), until);
                }
                None => {
                    self.0.remove(&icon.identifier);
                }
            }
        }
    }

    pub fn is_missing(&self, identifier: &str, now: Instant) -> bool {
        self.0.get(identifier).is_some_and(|until| *until > now)
    }
}

struct RequestBatch {
    generation: u64,
    requests: Vec<IconRequest>,
}

#[derive(Default)]
struct Pending {
    shell: Option<RequestBatch>,
    websites: Option<RequestBatch>,
    shutdown: bool,
}

/// Each worker thread takes requests from its own queue.
#[derive(Clone, Copy)]
enum Queue {
    /// Windows Shell icons: local, and never the network.
    Shell,
    /// Website favicons, which may wait seconds for a slow site.
    Websites,
}

impl Queue {
    fn batch(self, pending: &mut Pending) -> &mut Option<RequestBatch> {
        match self {
            Self::Shell => &mut pending.shell,
            Self::Websites => &mut pending.websites,
        }
    }
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
    changed: Condvar,
    generation: AtomicU64,
    completed: Mutex<Option<Vec<LoadedIcon>>>,
}

/// Two threads, so a slow website never delays application icons: one extracts Shell icons,
/// the other fetches website icons. Both publish to the same completed list.
pub struct IconWorker {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

impl IconWorker {
    pub fn new(window: HWND, allow_network: bool) -> std::io::Result<Self> {
        let address = window.0 as usize;
        // Dropped on a failed spawn, which stops a thread already started.
        let mut worker = Self {
            shared: Arc::new(Shared::default()),
            threads: Vec::with_capacity(2),
        };
        let shared = worker.shared.clone();
        worker.threads.push(
            thread::Builder::new()
                .name("core-icons".into())
                .spawn(move || run_shell(address, shared))?,
        );
        let shared = worker.shared.clone();
        worker.threads.push(
            thread::Builder::new()
                .name("core-favicons".into())
                .spawn(move || run_websites(address, shared, allow_network))?,
        );
        Ok(worker)
    }

    pub fn submit(&self, requests: Vec<IconRequest>) {
        let generation = self.shared.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let (websites, shell): (Vec<_>, Vec<_>) = requests
            .into_iter()
            .partition(|request| matches!(request.source, IconSource::Website(_)));
        let should_wake = !shell.is_empty() || !websites.is_empty();
        let batch = |requests: Vec<IconRequest>| {
            (!requests.is_empty()).then_some(RequestBatch {
                generation,
                requests,
            })
        };
        let mut pending = self.shared.pending.lock().expect("icon request lock");
        pending.shell = batch(shell);
        pending.websites = batch(websites);
        drop(pending);
        if should_wake {
            self.shared.changed.notify_all();
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
            pending.shell = None;
            pending.websites = None;
            self.shared.generation.fetch_add(1, Ordering::AcqRel);
        }
        self.shared.changed.notify_all();
        // A third-party Shell extension or a website may stall. Never block the UI on that at
        // exit. The shared shutdown flag prevents later posts; thread-local icons drop on return.
        for thread in self.threads.drain(..).filter(JoinHandle::is_finished) {
            if thread.join().is_err() {
                eprintln!("Core icon worker stopped unexpectedly");
            }
        }
    }
}

fn run_shell(address: usize, shared: Arc<Shared>) {
    let mut cache = VecDeque::with_capacity(CACHE_CAPACITY);
    // Initialize lazily so a hidden launcher does not load the Shell icon machinery.
    let mut com = None;
    while let Some(batch) = next_batch(&shared, Queue::Shell) {
        if com.is_none() {
            match ComApartment::new() {
                Ok(apartment) => com = Some(apartment),
                Err(error) => {
                    eprintln!("Application icons unavailable: {error}");
                    return;
                }
            }
        }
        let current = || shared.generation.load(Ordering::Acquire) == batch.generation;
        // Icons loaded before appear at once; each new one appears as soon as it is extracted,
        // not after the slowest in the batch.
        let mut loaded = Vec::new();
        let mut missing = Vec::new();
        for request in batch.requests {
            match cached(&mut cache, &request.identifier) {
                Some(icon) => loaded.push(icon),
                None => missing.push(request),
            }
        }
        publish(address, &shared, loaded);
        for request in missing {
            if !current() {
                break;
            }
            // Never the network, even for a website request that reached this queue.
            publish(
                address,
                &shared,
                vec![cached_icon(&mut cache, request, false)],
            );
        }
    }
    // Release icons before the COM apartment is uninitialized.
    drop(cache);
}

fn run_websites(address: usize, shared: Arc<Shared>, allow_network: bool) {
    let mut cache = VecDeque::with_capacity(CACHE_CAPACITY);
    while let Some(batch) = next_batch(&shared, Queue::Websites) {
        let current = || shared.generation.load(Ordering::Acquire) == batch.generation;
        // Downloads are slow and re-renders (search results, exchange rates, discovery) start
        // new batches often, so each website icon is published the moment it is ready.
        for request in batch.requests {
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
}

fn next_batch(shared: &Shared, queue: Queue) -> Option<RequestBatch> {
    let mut pending = shared.pending.lock().expect("icon request lock");
    loop {
        if pending.shutdown {
            return None;
        }
        if let Some(batch) = queue.batch(&mut pending).take() {
            return Some(batch);
        }
        pending = shared.changed.wait(pending).expect("icon wake lock");
    }
}

/// A loaded icon, or a failure not yet due for a retry, moved to the most recent place.
fn cached(cache: &mut VecDeque<LoadedIcon>, identifier: &str) -> Option<LoadedIcon> {
    let index = cache
        .iter()
        .position(|entry| &*entry.identifier == identifier)?;
    let entry = cache.remove(index).expect("cached icon index");
    if entry.icon.is_none() && entry.loaded_at.elapsed() >= FAILED_ICON_RETRY {
        return None;
    }
    cache.push_back(entry.clone());
    Some(entry)
}

fn cached_icon(
    cache: &mut VecDeque<LoadedIcon>,
    request: IconRequest,
    allow_network: bool,
) -> LoadedIcon {
    if let Some(entry) = cached(cache, &request.identifier) {
        return entry;
    }
    let loaded = LoadedIcon {
        icon: request.source.load(allow_network).map(Arc::new),
        identifier: request.identifier,
        loaded_at: Instant::now(),
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
    fn the_ui_skips_failed_icons_until_the_worker_would_retry_them() {
        // Loading a real icon must not disturb the handle-counting test.
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _apartment = ComApartment::new().unwrap();
        let icon = Arc::new(
            ApplicationIcon::load(&std::env::current_exe().unwrap()).expect("executable icon"),
        );
        let start = Instant::now();
        let loaded = |identifier: &str, found: bool, loaded_at: Instant| LoadedIcon {
            identifier: identifier.into(),
            icon: found.then(|| icon.clone()),
            loaded_at,
        };
        let mut missing = MissingIcons::default();
        missing.record(
            &[loaded("broken", false, start), loaded("fine", true, start)],
            start,
        );
        assert!(missing.is_missing("broken", start));
        assert!(missing.is_missing("broken", start + FAILED_ICON_RETRY / 2));
        assert!(!missing.is_missing("fine", start));
        assert!(!missing.is_missing("unknown", start));
        // Retry time: asked for again.
        assert!(!missing.is_missing("broken", start + FAILED_ICON_RETRY));
        // A failure the worker cached earlier only waits out the rest of its retry time.
        let later = start + FAILED_ICON_RETRY / 2;
        missing.record(&[loaded("stale", false, start)], later);
        assert!(!missing.is_missing("stale", start + FAILED_ICON_RETRY));
        // A later success clears the failure, and expired entries are dropped.
        missing.record(&[loaded("broken", true, later)], later);
        assert!(!missing.is_missing("broken", later));
        missing.record(&[], start + FAILED_ICON_RETRY * 2);
        assert!(missing.0.is_empty());
    }

    /// Contacts the network: run with `cargo test -- --ignored stalled`.
    #[test]
    #[ignore = "requires network access; waits for a connection that never answers"]
    fn application_icons_arrive_while_a_website_request_is_stalled() {
        // The workers post to a null window and load an icon; keep them off handle counts.
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let worker = IconWorker::new(HWND::default(), true).unwrap();
        // Unroutable: the connection waits for WinHTTP's 3 s connect timeout.
        worker.submit(vec![IconRequest {
            identifier: "stalled".into(),
            source: IconSource::Website(WebsiteOrigin::parse("http://10.255.255.1").unwrap()),
        }]);
        thread::sleep(Duration::from_millis(300));
        let started = Instant::now();
        worker.submit(vec![IconRequest {
            identifier: "app".into(),
            source: IconSource::Shell(std::env::current_exe().unwrap()),
        }]);
        let icon = loop {
            if let Some(icon) = worker
                .take_completed()
                .and_then(|icons| icons.into_iter().find(|icon| &*icon.identifier == "app"))
            {
                break icon;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "no application icon"
            );
            thread::sleep(Duration::from_millis(5));
        };
        assert!(icon.icon.is_some());
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the application icon waited {:?} behind the website",
            started.elapsed()
        );
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
