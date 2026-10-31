use super::discover_applications::{discover_applications, Discovery};
use core_engine::{
    applications::ApplicationCatalog,
    conversions::ExchangeRates,
    media::{MediaState, MixerApp},
    quicklinks::Quicklink,
    search::{AppInfo, SearchBatch, SearchEngine},
};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Condvar, Mutex,
};
use std::thread::{self, JoinHandle};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

pub const WORKER_READY: u32 = WM_APP + 1;

/// What a search uses besides its text: shared snapshots, cheap to hand over.
pub struct SearchContext {
    pub catalog: Arc<ApplicationCatalog>,
    pub quicklinks: Arc<[Quicklink]>,
    pub exchange_rates: Option<Arc<ExchangeRates>>,
    pub recent_applications: Arc<[Arc<str>]>,
    pub media: Option<Arc<MediaState>>,
    /// Windows' volume mixer for `@volume`; None until it has been read.
    pub mixer: Option<Arc<[MixerApp]>>,
    pub app_info: Option<Arc<AppInfo>>,
    pub songs: Option<Arc<core_engine::search::SongSearch>>,
    /// The person's Spotify playlists for `@playlist`; None until they were asked for.
    pub playlists: Option<Arc<core_engine::search::PlaylistLibrary>>,
}

struct Request {
    generation: u64,
    query: String,
    context: SearchContext,
}
pub struct Completion {
    pub generation: u64,
    pub batch: SearchBatch,
}
#[derive(Default)]
struct Pending {
    request: Option<Request>,
    discovering: bool,
    shutdown: bool,
}

#[derive(Default)]
pub struct Events {
    pub catalog: Mutex<Option<Discovery>>,
    pub results: Mutex<Option<Completion>>,
}

pub struct SearchWorker {
    pending: Arc<(Mutex<Pending>, Condvar)>,
    pub events: Arc<Events>,
    pub stopped: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    search_thread: Option<JoinHandle<()>>,
}

impl SearchWorker {
    pub fn new(window: HWND, discover: bool) -> std::io::Result<Self> {
        let pending = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let events = Arc::new(Events::default());
        let stopped = Arc::new(AtomicBool::new(false));
        let generation = Arc::new(AtomicU64::new(0));
        let address = window.0 as usize;
        let search_thread = {
            let (pending, events, stopped, generation) = (
                pending.clone(),
                events.clone(),
                stopped.clone(),
                generation.clone(),
            );
            thread::Builder::new()
                .name("core-search".into())
                .spawn(move || run_search(address, pending, events, stopped, generation))?
        };
        let worker = Self {
            pending,
            events,
            stopped,
            generation,
            search_thread: Some(search_thread),
        };
        if discover {
            worker.refresh_catalog(window)?;
        }
        Ok(worker)
    }

    /// Rescan on a separate thread; repeated requests share the scan already in progress.
    pub fn refresh_catalog(&self, window: HWND) -> std::io::Result<()> {
        self.start_discovery(window, discover_applications)
    }

    fn start_discovery(
        &self,
        window: HWND,
        discover: impl FnOnce(&AtomicBool) -> Discovery + Send + 'static,
    ) -> std::io::Result<()> {
        let mut state = self.pending.0.lock().expect("search request lock");
        if state.shutdown || state.discovering {
            return Ok(());
        }
        state.discovering = true;
        drop(state);
        let (pending, events, stopped) = (
            self.pending.clone(),
            self.events.clone(),
            self.stopped.clone(),
        );
        let address = window.0 as usize;
        // Detached: Shell enumeration can block, and exit must not wait for it.
        let spawned = thread::Builder::new()
            .name("core-discovery".into())
            .spawn(move || {
                let catalog = discover(&stopped);
                // Drop sets `shutdown` under this lock, so no wake can follow it.
                let mut state = pending.0.lock().expect("search request lock");
                state.discovering = false;
                if !state.shutdown {
                    *events.catalog.lock().expect("catalog event lock") = Some(catalog);
                    wake(address);
                }
            });
        if let Err(error) = spawned {
            self.pending
                .0
                .lock()
                .expect("search request lock")
                .discovering = false;
            return Err(error);
        }
        Ok(())
    }

    pub fn submit(&self, generation: u64, query: String, context: SearchContext) {
        self.generation.store(generation, Ordering::Release);
        let (lock, changed) = &*self.pending;
        lock.lock().expect("search request lock").request = Some(Request {
            generation,
            query,
            context,
        });
        changed.notify_one();
    }

    pub fn cancel(&self, generation: u64) {
        self.generation.store(generation, Ordering::Release);
        self.pending.0.lock().expect("search request lock").request = None;
        self.events
            .results
            .lock()
            .expect("search results lock")
            .take();
    }
}

impl Drop for SearchWorker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        self.pending.0.lock().expect("search request lock").shutdown = true;
        self.pending.1.notify_one();
        if let Some(thread) = self.search_thread.take() {
            if thread.join().is_err() {
                eprintln!("Core's search worker stopped unexpectedly");
            }
        }
    }
}

fn run_search(
    address: usize,
    pending: Arc<(Mutex<Pending>, Condvar)>,
    events: Arc<Events>,
    stopped: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
) {
    let mut engine =
        SearchEngine::with_time_converter(super::time_converter::WindowsTimeConverter::default())
            .with_calendar_clock(super::time_converter::WindowsCalendarClock);
    let mut source_quicklinks: Arc<[Quicklink]> = Arc::from([]);
    let mut quicklink_catalog = ApplicationCatalog::default();
    loop {
        let request = {
            let (lock, changed) = &*pending;
            let mut state = lock.lock().expect("search request lock");
            while state.request.is_none() && !state.shutdown {
                state = changed.wait(state).expect("search wake lock");
            }
            if state.shutdown {
                return;
            }
            state.request.take().expect("pending search")
        };
        if generation.load(Ordering::Acquire) != request.generation {
            continue;
        }
        // Quicklinks are ranked in their own catalog and merged with the apps' results, so only
        // a quicklink edit rebuilds a catalog here; new apps from discovery are used as they are.
        if !Arc::ptr_eq(&source_quicklinks, &request.context.quicklinks) {
            quicklink_catalog = if request.context.quicklinks.is_empty() {
                ApplicationCatalog::default()
            } else {
                ApplicationCatalog::new(
                    request
                        .context
                        .quicklinks
                        .iter()
                        .map(Quicklink::application)
                        .collect(),
                )
            };
            source_quicklinks = request.context.quicklinks.clone();
        }
        engine.set_exchange_rates(request.context.exchange_rates.clone());
        engine.set_recent_applications(request.context.recent_applications.clone());
        engine.set_media(request.context.media.clone());
        engine.set_mixer(request.context.mixer.clone());
        engine.set_app_info(request.context.app_info.clone());
        engine.set_songs(request.context.songs.clone());
        engine.set_playlists(request.context.playlists.clone());
        let batch = engine.search_catalogs(
            &request.query,
            &request.context.catalog,
            &request.context.catalog,
            &quicklink_catalog,
            &|| {
                stopped.load(Ordering::Relaxed)
                    || generation.load(Ordering::Relaxed) != request.generation
            },
        );
        if stopped.load(Ordering::Acquire)
            || generation.load(Ordering::Acquire) != request.generation
        {
            continue;
        }
        let mut results = events.results.lock().expect("search results lock");
        // Cancellation clears this mailbox under the same lock; recheck before publishing.
        if stopped.load(Ordering::Acquire)
            || generation.load(Ordering::Acquire) != request.generation
        {
            continue;
        }
        let should_wake = results
            .replace(Completion {
                generation: request.generation,
                batch,
            })
            .is_none();
        drop(results);
        if should_wake {
            wake(address);
        }
    }
}

fn wake(address: usize) {
    if let Err(error) = unsafe {
        PostMessageW(
            Some(HWND(address as *mut _)),
            WORKER_READY,
            WPARAM(0),
            LPARAM(0),
        )
    } {
        eprintln!("Could not notify the Core window: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_engine::applications::Application;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn context(catalog: &Arc<ApplicationCatalog>, quicklinks: &Arc<[Quicklink]>) -> SearchContext {
        SearchContext {
            catalog: catalog.clone(),
            quicklinks: quicklinks.clone(),
            exchange_rates: None,
            recent_applications: Arc::from([]),
            media: None,
            mixer: None,
            songs: None,
            playlists: None,
            app_info: None,
        }
    }

    fn next_completion(worker: &SearchWorker) -> Completion {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(completion) = worker.events.results.lock().unwrap().take() {
                return completion;
            }
            assert!(Instant::now() < deadline, "search worker did not respond");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn discovery(name: &str) -> Discovery {
        let identifier: Arc<str> = name.into();
        Discovery {
            catalog: Arc::new(ApplicationCatalog::new(vec![Application {
                id: identifier.clone(),
                name: name.into(),
                description: "Discovery test".into(),
                pinned: false,
                launches: 0,
                aliases: Arc::from([]),
            }])),
            targets: [(identifier, std::path::PathBuf::from(format!("{name}.lnk")))].into(),
            aliases: Default::default(),
            warnings: Vec::new(),
        }
    }

    fn wait_for_discovery(pending: &Mutex<Pending>) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while pending.lock().unwrap().discovering {
            assert!(Instant::now() < deadline, "discovery did not finish");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn catalog_refreshes_are_serial_and_do_not_block_search() {
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let worker = SearchWorker::new(HWND::default(), false).unwrap();
        let (started, entered) = mpsc::channel();
        let (finish, released) = mpsc::channel();
        worker
            .start_discovery(HWND::default(), move |_| {
                started.send(()).unwrap();
                released.recv().unwrap();
                discovery("Existing")
            })
            .unwrap();
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        let (duplicate, unexpected) = mpsc::channel();
        worker
            .start_discovery(HWND::default(), move |_| {
                duplicate.send(()).unwrap();
                discovery("Duplicate")
            })
            .unwrap();
        assert!(matches!(
            unexpected.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));

        let catalog = Arc::new(ApplicationCatalog::default());
        let quicklinks: Arc<[Quicklink]> = Arc::from([]);
        worker.submit(1, "2+2".into(), context(&catalog, &quicklinks));
        assert_eq!(
            next_completion(&worker).batch.results[0].title.as_ref(),
            "4"
        );
        finish.send(()).unwrap();
        wait_for_discovery(&worker.pending.0);
        let first = worker.events.catalog.lock().unwrap().take().unwrap();
        assert!(first.targets.contains_key("Existing"));

        worker
            .start_discovery(HWND::default(), |_| discovery("Installed"))
            .unwrap();
        wait_for_discovery(&worker.pending.0);
        let refreshed = worker.events.catalog.lock().unwrap().take().unwrap();
        assert!(refreshed.targets.contains_key("Installed"));
        assert!(!refreshed.targets.contains_key("Existing"));
        worker.submit(
            2,
            "@app Installed".into(),
            context(&refreshed.catalog, &quicklinks),
        );
        assert_eq!(
            next_completion(&worker).batch.results[0].title.as_ref(),
            "Installed"
        );
    }

    #[test]
    fn shutdown_does_not_wait_for_discovery_or_publish_to_a_closed_window() {
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let worker = SearchWorker::new(HWND::default(), false).unwrap();
        let (started, entered) = mpsc::channel();
        let (finish, released) = mpsc::channel();
        worker
            .start_discovery(HWND::default(), move |_| {
                started.send(()).unwrap();
                released.recv().unwrap();
                discovery("Too late")
            })
            .unwrap();
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        let pending = worker.pending.clone();
        let events = worker.events.clone();

        drop(worker);
        assert!(pending.0.lock().unwrap().shutdown);
        finish.send(()).unwrap();
        wait_for_discovery(&pending.0);
        assert!(events.catalog.lock().unwrap().is_none());
    }

    #[test]
    fn cancellation_discards_a_result_that_finished_before_invalidation() {
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let worker = SearchWorker::new(HWND::default(), false).unwrap();
        let catalog = Arc::new(ApplicationCatalog::default());
        let quicklinks: Arc<[Quicklink]> = Arc::from([]);
        worker.submit(1, "2+2".into(), context(&catalog, &quicklinks));
        let deadline = Instant::now() + Duration::from_secs(10);
        while worker.events.results.lock().unwrap().is_none() {
            assert!(Instant::now() < deadline, "search worker did not respond");
            thread::sleep(Duration::from_millis(5));
        }

        worker.cancel(2);

        assert!(worker.events.results.lock().unwrap().is_none());
    }

    #[test]
    fn the_latest_request_wins_and_cancellation_drops_pending_work() {
        // The worker thread gains a message queue when it posts; keep it off handle counts.
        let _serial = crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // A null window makes completion notifications harmless thread messages.
        let worker = SearchWorker::new(HWND::default(), false).unwrap();
        let catalog = Arc::new(ApplicationCatalog::default());
        let quicklinks: Arc<[Quicklink]> = Arc::from([]);
        for (generation, query) in [(1, "1+1"), (2, "2+2"), (3, "3+3")] {
            worker.submit(generation, query.into(), context(&catalog, &quicklinks));
        }
        let latest = loop {
            let completion = next_completion(&worker);
            assert!(completion.generation <= 3);
            if completion.generation == 3 {
                break completion;
            }
        };
        assert_eq!(latest.batch.results[0].title.as_ref(), "6");

        worker.submit(4, "4+4".into(), context(&catalog, &quicklinks));
        worker.cancel(5);
        assert!(worker.pending.0.lock().unwrap().request.is_none());
        thread::sleep(Duration::from_millis(200));
        // A result for the cancelled generation must never be published.
        assert!(worker.events.results.lock().unwrap().is_none());
    }
}
