//! The media worker: reads sessions when asked, sends commands, and while Core is visible
//! subscribes to the players' change events so the bar and results stay current.
use super::{
    album_art::ArtCache,
    post_ready,
    read_sessions::{read_sessions, read_timeline, ReadSession, SessionRead},
    send_command, window_players, MediaOutcome, MediaReading, Shared,
};
use core_engine::media::MediaSession;
use std::{sync::Arc, thread, time::Duration};
use windows::{
    core::{Interface, Result, RuntimeType},
    Foundation::TypedEventHandler,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as SessionManager,
    },
    Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
};

/// Players announce one change as a burst of events; wait for it to end before reading.
const EVENT_SETTLE: Duration = Duration::from_millis(60);
const MANAGER_TIMEOUT: Duration = Duration::from_secs(5);
/// After a command, players take a moment to report their new state.
const COMMAND_SETTLE: Duration = Duration::from_millis(150);

pub(super) fn run(address: usize, shared: Arc<Shared>) {
    // Its own multithreaded apartment: session events arrive on Windows' thread pool.
    let initialized = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok();
    Worker {
        address,
        shared,
        manager: None,
        watch: None,
        sessions: Vec::new(),
        art: ArtCache::default(),
    }
    .serve();
    if initialized {
        unsafe { RoUninitialize() };
    }
}

struct Worker {
    address: usize,
    shared: Arc<Shared>,
    /// None until first needed, and again after Windows refused it.
    manager: Option<SessionManager>,
    watch: Option<Watch>,
    /// The sessions of the last full reading, for reading track positions alone.
    sessions: Vec<(MediaSession, Session)>,
    art: ArtCache,
}

impl Worker {
    fn serve(mut self) {
        loop {
            let Some(work) = self.next_work() else {
                return;
            };
            let manager = self.manager();
            let mut outcomes = Vec::new();
            for request in &work.requests {
                outcomes.push(send_command::execute(manager.as_ref(), request));
            }
            self.set_watching(work.watch, manager.as_ref());
            // After a command, read again even while hidden: Core then knows whether the player
            // it paused is still open when it is next shown.
            if !outcomes.is_empty() {
                thread::sleep(COMMAND_SETTLE);
            }
            let full = work.requested || work.changed || !outcomes.is_empty();
            let reading = if full {
                Some(self.read(manager.as_ref(), work.art_edge))
            } else if work.timelines {
                self.read_timelines()
            } else {
                None
            };
            if !self.publish(reading, outcomes) {
                return;
            }
        }
    }

    /// Waits for work. Work announced only by events waits for the burst to settle first.
    fn next_work(&self) -> Option<super::Work> {
        let mut work = self.shared.work.lock().expect("media work lock");
        while !work.pending() && !work.shutdown {
            work = self.shared.wake.wait(work).expect("media wake lock");
        }
        if !work.requested && work.requests.is_empty() && !work.shutdown {
            drop(work);
            thread::sleep(EVENT_SETTLE);
            work = self.shared.work.lock().expect("media work lock");
        }
        if work.shutdown {
            return None;
        }
        let taken = super::Work {
            requested: work.requested,
            changed: work.changed,
            timelines: work.timelines,
            watch: work.watch,
            watch_changed: work.watch_changed,
            art_edge: work.art_edge,
            requests: std::mem::take(&mut work.requests),
            shutdown: false,
        };
        work.requested = false;
        work.changed = false;
        work.timelines = false;
        work.watch_changed = false;
        Some(taken)
    }

    fn manager(&mut self) -> Option<SessionManager> {
        if self.manager.is_none() {
            match request_manager() {
                Ok(manager) => self.manager = Some(manager),
                Err(error) => eprintln!("Windows media sessions are unavailable: {error}"),
            }
        }
        self.manager.clone()
    }

    fn set_watching(&mut self, watch: bool, manager: Option<&SessionManager>) {
        if !watch {
            self.watch = None;
            return;
        }
        if self.watch.is_some() {
            return;
        }
        if let Some(manager) = manager {
            match Watch::start(manager, &self.shared) {
                Ok(started) => self.watch = Some(started),
                Err(error) => eprintln!("Could not watch media sessions: {error}"),
            }
        }
    }

    fn read(&mut self, manager: Option<&SessionManager>, art_edge: u32) -> MediaReading {
        let empty = || SessionRead {
            kept: Vec::new(),
            all: Vec::new(),
        };
        let SessionRead { kept: read, all } =
            match manager.map(|manager| read_sessions(manager, true)) {
                Some(Ok(read)) => read,
                Some(Err(error)) => {
                    eprintln!("Could not read media sessions: {error}");
                    empty()
                }
                None => empty(),
            };
        let art = if art_edge == 0 {
            Vec::new()
        } else {
            read.iter()
                .filter_map(|entry| {
                    let icon = self.art.get(entry, art_edge)?;
                    Some((entry.info.app_id.clone(), icon))
                })
                .collect()
        };
        if let Some(watching) = &mut self.watch {
            watching.follow(&all, &self.shared);
        }
        self.sessions = read
            .iter()
            .map(|entry| (entry.info.clone(), entry.session.clone()))
            .collect();
        let mut sessions: Vec<MediaSession> = read
            .into_iter()
            .map(|entry: ReadSession| entry.info)
            .collect();
        let players = window_players::beside(&sessions);
        let window_processes = players.iter().map(|player| player.process).collect();
        sessions.extend(players.into_iter().map(|player| player.info));
        MediaReading {
            unavailable: manager.is_none() && sessions.is_empty(),
            sessions,
            art,
            timeline_only: false,
            window_processes,
        }
    }

    fn read_timelines(&mut self) -> Option<MediaReading> {
        if self.sessions.is_empty() {
            return None;
        }
        for (info, session) in &mut self.sessions {
            info.timeline = read_timeline(session);
        }
        Some(MediaReading {
            sessions: self.sessions.iter().map(|(info, _)| info.clone()).collect(),
            art: Vec::new(),
            unavailable: false,
            timeline_only: true,
            window_processes: Vec::new(),
        })
    }

    /// False once Core has shut the service down.
    fn publish(&self, reading: Option<MediaReading>, outcomes: Vec<MediaOutcome>) -> bool {
        if self.shared.work.lock().expect("media work lock").shutdown {
            return false;
        }
        let mut notify = false;
        if let Some(reading) = reading {
            let mut slot = self.shared.reading.lock().expect("media reading lock");
            match slot.as_mut() {
                // A full reading Core has not taken yet keeps its text and art.
                Some(waiting) if reading.timeline_only => {
                    for session in &mut waiting.sessions {
                        if let Some(update) = reading
                            .sessions
                            .iter()
                            .find(|update| update.app_id == session.app_id)
                        {
                            session.timeline = update.timeline;
                        }
                    }
                }
                Some(waiting) => *waiting = reading,
                None => {
                    *slot = Some(reading);
                    notify = true;
                }
            }
        }
        if !outcomes.is_empty() {
            let mut slot = self.shared.outcomes.lock().expect("media outcome lock");
            notify |= slot.is_empty();
            slot.extend(outcomes);
        }
        if notify {
            post_ready(self.address);
        }
        true
    }
}

fn request_manager() -> Result<SessionManager> {
    finish!(SessionManager::RequestAsync()?, MANAGER_TIMEOUT)
}

/// Change subscriptions, held only while Core is visible. Dropping one unsubscribes.
struct Watch {
    manager: SessionManager,
    manager_tokens: [i64; 2],
    sessions: Vec<(Session, [i64; 3])>,
}

impl Watch {
    fn start(manager: &SessionManager, shared: &Arc<Shared>) -> Result<Self> {
        let sessions_changed = manager.SessionsChanged(&announce(shared, false))?;
        let current_changed = match manager.CurrentSessionChanged(&announce(shared, false)) {
            Ok(token) => token,
            Err(error) => {
                let _ = manager.RemoveSessionsChanged(sessions_changed);
                return Err(error);
            }
        };
        Ok(Self {
            manager: manager.clone(),
            manager_tokens: [sessions_changed, current_changed],
            sessions: Vec::new(),
        })
    }

    /// Subscribes to sessions that appeared and drops those that closed. Every session is
    /// watched, including each tab a browser reports under one identifier, matched by object.
    fn follow(&mut self, all: &[Session], shared: &Arc<Shared>) {
        let same = |first: &Session, second: &Session| first.as_raw() == second.as_raw();
        self.sessions.retain(|(session, tokens)| {
            let open = all.iter().any(|current| same(current, session));
            if !open {
                unsubscribe(session, *tokens);
            }
            open
        });
        for session in all {
            if self
                .sessions
                .iter()
                .any(|(watched, _)| same(watched, session))
            {
                continue;
            }
            match subscribe(session, shared) {
                Ok(tokens) => self.sessions.push((session.clone(), tokens)),
                Err(error) => eprintln!("Could not watch a media session: {error}"),
            }
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        for (session, tokens) in &self.sessions {
            unsubscribe(session, *tokens);
        }
        let [sessions_changed, current_changed] = self.manager_tokens;
        let _ = self.manager.RemoveSessionsChanged(sessions_changed);
        let _ = self.manager.RemoveCurrentSessionChanged(current_changed);
    }
}

fn subscribe(session: &Session, shared: &Arc<Shared>) -> Result<[i64; 3]> {
    let playback = session.PlaybackInfoChanged(&announce(shared, false))?;
    let properties = session.MediaPropertiesChanged(&announce(shared, false));
    let timeline = session.TimelinePropertiesChanged(&announce(shared, true));
    match (properties, timeline) {
        (Ok(properties), Ok(timeline)) => Ok([playback, properties, timeline]),
        (properties, timeline) => {
            let _ = session.RemovePlaybackInfoChanged(playback);
            if let Ok(token) = properties {
                let _ = session.RemoveMediaPropertiesChanged(token);
            }
            if let Ok(token) = timeline {
                let _ = session.RemoveTimelinePropertiesChanged(token);
            }
            Err(windows::core::Error::new(
                windows::Win32::Foundation::E_FAIL,
                "The player refused change notifications.",
            ))
        }
    }
}

fn unsubscribe(session: &Session, [playback, properties, timeline]: [i64; 3]) {
    let _ = session.RemovePlaybackInfoChanged(playback);
    let _ = session.RemoveMediaPropertiesChanged(properties);
    let _ = session.RemoveTimelinePropertiesChanged(timeline);
}

/// A handler that only tells the worker; reading happens on the worker's own thread.
fn announce<Sender: RuntimeType + 'static, Arguments: RuntimeType + 'static>(
    shared: &Arc<Shared>,
    timeline_only: bool,
) -> TypedEventHandler<Sender, Arguments> {
    let shared = Arc::downgrade(shared);
    TypedEventHandler::new(move |_, _| {
        if let Some(shared) = shared.upgrade() {
            shared.announce(timeline_only);
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prints what Windows reports for every open player, with its art decoded as the bar
    /// would. Run by hand: `cargo test -p core-launcher-v2 media_probe -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads the media sessions of the person's own players"]
    fn media_probe() {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let manager = request_manager().expect("Windows media sessions");
        let read = read_sessions(&manager, true).expect("media sessions").kept;
        println!("{} session(s)", read.len());
        let mut art = ArtCache::default();
        for entry in &read {
            let info = &entry.info;
            let progress = info
                .timeline
                .and_then(|timeline| core_engine::media::playback_position(timeline, false, 0));
            println!(
                "{:?}\n  name {:?} class {:?} state {:?} current {}\n  title {:?} artist {:?} album {:?}\n  controls {:?}\n  timeline {:?} progress {:?}\n  art {}",
                info.app_id,
                info.app_name,
                info.class,
                info.state,
                info.is_system_current,
                info.title,
                info.artist,
                info.album,
                info.controls,
                info.timeline,
                progress,
                if art.get(entry, 60).is_some() { "decoded" } else { "none" },
            );
        }
    }
}
