use super::{
    info::{self, AppInfo},
    media, mixer, playlists,
    power::power_results,
    recent_applications::recent_results,
    run_target, songs, taskbar, terminal, PowerAction, RunMode, ShellKind,
};
use super::{parse_query, CommandKind, ParsedQuery};
use crate::{
    applications::{ApplicationCatalog, SearchScratch},
    calculator::{
        calendar::{parse_calendar, CalendarClock},
        format_number, Calculation, CalculatorEngine,
    },
    conversions::{self, Conversion, ConversionContext, ExchangeRates},
    media::{MediaCommand, MediaState, MixerApp, VolumeLevel},
    time_conversion::{parse_time, recognizes_time, TimeConverter, TimeError, TimeRequest},
    VISIBLE_RESULT_LIMIT,
};
use std::sync::Arc;

const APPLICATION_MESSAGE: &str = "Enter to open · ↑ ↓ to select · Esc to hide";
/// Empty-query lists kept at once: apps, apps with quicklinks, and quicklinks alone.
const EMPTY_QUERY_CATALOGS: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    PlaySong(super::Song),
    /// A Spotify playlist, album or artist, played from its start.
    PlayCollection(super::Collection),
    LaunchApplication(Arc<str>),
    CopyText(Arc<str>),
    OpenUrl(Arc<str>),
    OpenQuicklink(Arc<str>),
    FillQuery(Arc<str>),
    Power(PowerAction),
    RevealTaskbar,
    Update,
    RunCommand {
        command: Arc<str>,
        shell: ShellKind,
        mode: RunMode,
    },
    OpenRunTarget {
        target: Arc<str>,
        elevated: bool,
    },
    /// A media control; with no target, the launcher chooses the player when it runs.
    Media {
        command: MediaCommand,
        target: Option<Arc<str>>,
    },
    /// A row of the volume mixer, with the level it showed. Enter mutes or unmutes it; the
    /// launcher's slider and the arrow keys change the level.
    Mixer {
        app: Arc<str>,
        level: VolumeLevel,
    },
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub kind: ResultKind,
    pub id: Arc<str>,
    pub title: Arc<str>,
    pub description: Arc<str>,
    pub action: Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultKind {
    Application,
    Quicklink,
    Calculator,
    Time,
    Date,
    Conversion,
    Power,
    Web,
    Command,
    System,
    Terminal,
    /// A recently used app, shown in the grid when nothing is typed.
    Recent,
    Media,
    /// A program in the volume mixer, drawn with a slider.
    Volume,
}

#[derive(Debug)]
pub struct SearchBatch {
    pub results: Vec<SearchResult>,
    pub message: &'static str,
}

#[derive(Default)]
pub struct SearchEngine {
    calculator: CalculatorEngine,
    scratch: SearchScratch,
    time_converter: Option<Box<dyn TimeConverter>>,
    calendar_clock: Option<Box<dyn CalendarClock>>,
    exchange_rates: Option<Arc<ExchangeRates>>,
    /// Media sessions and priorities; None until the launcher has read them.
    media: Option<Arc<MediaState>>,
    songs: Option<Arc<super::SongSearch>>,
    /// The person's Spotify playlists; None until the launcher has asked for them.
    playlists: Option<Arc<super::PlaylistLibrary>>,
    /// Windows' volume mixer; None until the launcher has read it for `@volume`.
    mixer: Option<Arc<[MixerApp]>>,
    /// Core's version and what the last update check saw, for `@info`.
    app_info: Option<Arc<AppInfo>>,
    /// Application identifiers, most recent first, shown when nothing is typed.
    recent_applications: Arc<[Arc<str>]>,
    /// Top results for an empty query per pair of catalog identities; a catalog never changes,
    /// so these stay valid until different catalogs are searched.
    empty_query_results: Vec<((u64, u64), Vec<SearchResult>)>,
}

impl SearchEngine {
    pub fn set_songs(&mut self, songs: Option<Arc<super::SongSearch>>) {
        self.songs = songs;
    }

    /// A snapshot of the person's Spotify playlists, replaced each time the launcher reads them.
    pub fn set_playlists(&mut self, playlists: Option<Arc<super::PlaylistLibrary>>) {
        self.playlists = playlists;
    }

    /// A snapshot of Windows' volume mixer, replaced each time the launcher reads it.
    pub fn set_mixer(&mut self, mixer: Option<Arc<[MixerApp]>>) {
        self.mixer = mixer;
    }
    pub fn with_calendar_clock(mut self, clock: impl CalendarClock + 'static) -> Self {
        self.calendar_clock = Some(Box::new(clock));
        self
    }
    /// Rates are a shared snapshot; the launcher replaces it when a newer ECB file arrives.
    pub fn set_exchange_rates(&mut self, rates: Option<Arc<ExchangeRates>>) {
        self.exchange_rates = rates;
    }

    /// A shared snapshot of media sessions, replaced when players or priorities change.
    pub fn set_media(&mut self, media: Option<Arc<MediaState>>) {
        self.media = media;
    }

    /// Core's version and update status, replaced when an update check ends.
    pub fn set_app_info(&mut self, app_info: Option<Arc<AppInfo>>) {
        self.app_info = app_info;
    }

    /// The launcher's recently used apps, most recent first; unknown identifiers are skipped.
    pub fn set_recent_applications(&mut self, recent: Arc<[Arc<str>]>) {
        self.recent_applications = recent;
    }

    pub fn with_time_converter(converter: impl TimeConverter + 'static) -> Self {
        Self {
            time_converter: Some(Box::new(converter)),
            ..Self::default()
        }
    }

    /// Search has no filesystem, network, clipboard or application-launch side effects.
    pub fn search(&mut self, query: &str, catalog: &ApplicationCatalog) -> SearchBatch {
        self.search_with_cancel(query, catalog, &|| false)
    }

    pub fn search_with_cancel(
        &mut self,
        query: &str,
        catalog: &ApplicationCatalog,
        cancelled: &impl Fn() -> bool,
    ) -> SearchBatch {
        self.search_catalogs(
            query,
            catalog,
            catalog,
            &ApplicationCatalog::default(),
            cancelled,
        )
    }

    /// `catalog` holds the apps: `@app` searches only them and recent apps come from them.
    /// Plain queries rank `general` and `quicklinks` as one list without a combined catalog;
    /// `>` searches only `quicklinks`.
    pub fn search_catalogs(
        &mut self,
        query: &str,
        catalog: &ApplicationCatalog,
        general: &ApplicationCatalog,
        quicklinks: &ApplicationCatalog,
        cancelled: &impl Fn() -> bool,
    ) -> SearchBatch {
        match parse_query(query) {
            ParsedQuery::Command {
                kind: CommandKind::Update,
                payload,
            } => update_results(payload),
            ParsedQuery::Search(payload) if PowerAction::parse(payload).is_some() => {
                power_results(payload)
            }
            ParsedQuery::Command {
                kind: CommandKind::Power,
                payload,
            } => power_results(payload),
            ParsedQuery::Search(payload) if taskbar::matches_keyword(payload) => {
                // Keep application matches (e.g. initials "tb") below the keyword result.
                let mut batch = self.applications(payload, general, quicklinks, cancelled);
                batch.results.insert(0, taskbar::taskbar_result());
                batch.results.truncate(VISIBLE_RESULT_LIMIT);
                batch.message = taskbar::MESSAGE;
                batch
            }
            ParsedQuery::Search(payload) if media::keyword(payload).is_some() => {
                let command = media::keyword(payload).expect("media keyword checked");
                let mut batch = self.applications(payload, general, quicklinks, cancelled);
                batch
                    .results
                    .insert(0, media::keyword_result(command, self.media.as_deref()));
                batch.results.truncate(VISIBLE_RESULT_LIMIT);
                batch.message = media::KEYWORD_MESSAGE;
                batch
            }
            ParsedQuery::Command {
                kind: CommandKind::Media,
                payload,
            } => media::media_results(payload, self.media.as_deref()),
            ParsedQuery::Command {
                kind: CommandKind::Info,
                payload,
            } => info::info_results(payload, self.app_info.as_deref()),
            ParsedQuery::Command {
                kind: kind @ (CommandKind::Songs | CommandKind::Albums | CommandKind::Artists),
                payload,
            } => songs::catalog_results(kind, payload, self.songs.as_deref()),
            ParsedQuery::Command {
                kind: CommandKind::Playlists,
                payload,
            } => playlists::playlist_results(payload, self.playlists.as_deref()),
            ParsedQuery::Command {
                kind: CommandKind::Mixer,
                payload,
            } => mixer::mixer_results(payload, self.mixer.as_deref()),
            ParsedQuery::Command {
                kind: CommandKind::Shell(shell),
                payload,
            } => terminal::terminal_results(payload, shell),
            ParsedQuery::Command {
                kind: CommandKind::Run,
                payload,
            } => run_target::run_results(payload, true),
            ParsedQuery::Search(payload) if run_target::is_run_target(payload) => {
                run_target::run_results(payload, false)
            }
            ParsedQuery::Command {
                kind: CommandKind::Taskbar,
                ..
            } => SearchBatch {
                results: vec![taskbar::taskbar_result()],
                message: taskbar::MESSAGE,
            },
            ParsedQuery::Search("") => recent_results(&self.recent_applications, catalog)
                .unwrap_or_else(|| self.applications("", general, quicklinks, cancelled)),
            ParsedQuery::Search(payload) => self
                .try_calculation(payload)
                .unwrap_or_else(|| self.applications(payload, general, quicklinks, cancelled)),
            ParsedQuery::Command {
                kind: CommandKind::Applications,
                payload,
            } => self.applications(payload, catalog, &ApplicationCatalog::default(), cancelled),
            ParsedQuery::Command {
                kind: CommandKind::Calculator,
                payload,
            } => self
                .try_calculation(payload)
                .unwrap_or_else(|| self.calculate(payload)),
            ParsedQuery::Command {
                kind: CommandKind::Time,
                payload,
            } => self.convert_time(payload),
            ParsedQuery::Command {
                kind: CommandKind::Web,
                payload,
            } => web_search(payload),
            ParsedQuery::Command {
                kind: CommandKind::Quicklinks,
                payload,
            } => {
                let mut batch = self.applications(
                    payload,
                    quicklinks,
                    &ApplicationCatalog::default(),
                    cancelled,
                );
                if quicklinks.is_empty() {
                    batch.message = "Add quicklinks in Settings → Quicklinks";
                }
                batch
            }
            ParsedQuery::CommandHints(prefix) => command_hints(prefix),
            ParsedQuery::UnknownCommand(_) => SearchBatch {
                results: Vec::new(),
                message: "Unknown command · try @app, @calc, @time or @web",
            },
            ParsedQuery::Invalid(_) => SearchBatch {
                results: Vec::new(),
                message: "Query contains invalid characters or exceeds 4 KiB",
            },
        }
    }

    fn try_calculation(&mut self, payload: &str) -> Option<SearchBatch> {
        if let Some(request) = parse_calendar(payload) {
            let message = if matches!(
                request,
                Ok(crate::calculator::calendar::CalendarRequest::Difference { .. })
            ) {
                "Enter to copy number of days · Esc to hide"
            } else {
                "Enter to copy date / time · Esc to hide"
            };
            return Some(
                match request.and_then(|request| request.evaluate(self.calendar_clock.as_deref())) {
                    Ok(answer) => calculation_batch(answer, ResultKind::Date, message),
                    Err(error) => SearchBatch {
                        results: Vec::new(),
                        message: error.message(),
                    },
                },
            );
        }
        // A complete time-zone query wins; `9 pt to cup` (pints) falls through to units.
        let time_request = recognizes_time(payload).then(|| parse_time(payload));
        if let Some(Ok(request)) = time_request {
            return Some(self.time_batch(Ok(request)));
        }
        let context = ConversionContext {
            rates: self.exchange_rates.as_deref(),
            clock: self.calendar_clock.as_deref(),
        };
        if let Some(outcome) = conversions::convert(payload, context) {
            return Some(match outcome {
                Ok(conversion) => conversion_batch(conversion),
                Err(message) => SearchBatch {
                    results: Vec::new(),
                    message,
                },
            });
        }
        if let Some(request) = time_request {
            return Some(self.time_batch(request));
        }
        self.calculator
            .recognizes(payload)
            .then(|| self.calculate(payload))
    }

    /// Ranks `catalog` and `quicklinks` as one list; either may be empty.
    fn applications(
        &mut self,
        query: &str,
        catalog: &ApplicationCatalog,
        quicklinks: &ApplicationCatalog,
        cancelled: &impl Fn() -> bool,
    ) -> SearchBatch {
        let empty_query = query.trim().is_empty();
        let identities = (catalog.identity(), quicklinks.identity());
        if empty_query {
            let cached = self
                .empty_query_results
                .iter()
                .find(|(cached, _)| *cached == identities);
            if let Some((_, results)) = cached {
                return SearchBatch {
                    results: results.clone(),
                    message: APPLICATION_MESSAGE,
                };
            }
        }
        let results: Vec<SearchResult> = catalog
            .search_merged(
                quicklinks,
                query,
                VISIBLE_RESULT_LIMIT,
                &mut self.scratch,
                cancelled,
            )
            .into_iter()
            .map(|ranked| {
                let application = ranked.application;
                let quicklink = application.id.starts_with(crate::quicklinks::ID_PREFIX);
                SearchResult {
                    kind: if quicklink {
                        ResultKind::Quicklink
                    } else {
                        ResultKind::Application
                    },
                    id: application.id.clone(),
                    title: application.name.clone(),
                    description: application.description.clone(),
                    action: if quicklink {
                        Action::OpenQuicklink(application.description.clone())
                    } else {
                        Action::LaunchApplication(application.id.clone())
                    },
                }
            })
            .collect();
        // A cancelled search returns nothing, which must not be remembered as the answer.
        if empty_query && !cancelled() {
            if self.empty_query_results.len() == EMPTY_QUERY_CATALOGS {
                self.empty_query_results.remove(0);
            }
            self.empty_query_results.push((identities, results.clone()));
        }
        SearchBatch {
            results,
            message: APPLICATION_MESSAGE,
        }
    }

    fn calculate(&self, payload: &str) -> SearchBatch {
        match self.calculator.evaluate(payload) {
            Ok(number) => {
                let text: Arc<str> = format_number(number).into();
                SearchBatch {
                    results: vec![SearchResult {
                        kind: ResultKind::Calculator,
                        id: "calculator".into(),
                        title: text.clone(),
                        description: format!("= {payload}").into(),
                        action: Action::CopyText(text),
                    }],
                    message: "Enter to copy answer · % divides by 100 · trig uses radians",
                }
            }
            Err(error) => SearchBatch {
                results: Vec::new(),
                message: match error {
                    crate::calculator::CalcError::Incomplete => "Finish the expression",
                    crate::calculator::CalcError::DivisionByZero => "Cannot divide by zero",
                    crate::calculator::CalcError::NonFinite => {
                        "Result is outside the supported numeric range"
                    }
                    crate::calculator::CalcError::TooComplex => {
                        "Expression is too long or deeply nested"
                    }
                    crate::calculator::CalcError::Invalid => {
                        "Try 2+2, sqrt(81), 2 days from now, or 10 kg to lb"
                    }
                    crate::calculator::CalcError::Domain => {
                        "That function is undefined for this input in real numbers"
                    }
                },
            },
        }
    }
}

/// Each answer is its own row; Enter copies that row's value.
fn conversion_batch(conversion: Conversion) -> SearchBatch {
    let results = conversion
        .answers
        .into_iter()
        .take(VISIBLE_RESULT_LIMIT)
        .enumerate()
        .map(|(index, answer)| SearchResult {
            kind: ResultKind::Conversion,
            id: format!("conversion-{index}").into(),
            title: answer.title.into(),
            description: answer.detail.into(),
            action: Action::CopyText(answer.copy.into()),
        })
        .collect();
    SearchBatch {
        results,
        message: conversion.message,
    }
}

fn calculation_batch(answer: Calculation, kind: ResultKind, message: &'static str) -> SearchBatch {
    SearchBatch {
        results: vec![SearchResult {
            kind,
            id: "calculation".into(),
            title: answer.title.into(),
            description: answer.detail.into(),
            action: Action::CopyText(answer.copy.into()),
        }],
        message,
    }
}

impl SearchEngine {
    fn convert_time(&mut self, payload: &str) -> SearchBatch {
        self.time_batch(parse_time(payload))
    }

    fn time_batch(&mut self, parsed: Result<TimeRequest, TimeError>) -> SearchBatch {
        let conversion = parsed.and_then(|request| {
            let converter = self.time_converter.as_mut().ok_or(TimeError::Unavailable)?;
            converter
                .convert(request)
                .map(|converted| (request, converted))
        });
        let (request, converted) = match conversion {
            Ok(conversion) => conversion,
            Err(error) => {
                return SearchBatch {
                    results: Vec::new(),
                    message: error.message(),
                }
            }
        };
        let day_change = match converted.destination_date.cmp(&converted.source_date) {
            std::cmp::Ordering::Greater => " · next day",
            std::cmp::Ordering::Less => " · previous day",
            std::cmp::Ordering::Equal => "",
        };
        let title = format!(
            "{} {}{day_change}",
            converted.time,
            request.destination.label()
        );
        let copied = format!(
            "{} {} on {}",
            converted.time,
            request.destination.label(),
            converted.destination_date
        );
        let description = format!(
            "{} {} on {} → {} · Enter to copy",
            request.time,
            request.source.label(),
            converted.source_date,
            converted.destination_date
        );
        SearchBatch {
            results: vec![SearchResult {
                kind: ResultKind::Time,
                id: "time-conversion".into(),
                title: title.into(),
                description: description.into(),
                action: Action::CopyText(copied.into()),
            }],
            message: if request.date.is_none() {
                "Using today in the source zone · add on YYYY-MM-DD for another date"
            } else {
                "Date-aware time conversion · Enter to copy"
            },
        }
    }
}

fn web_search(payload: &str) -> SearchBatch {
    if payload.is_empty() {
        return SearchBatch {
            results: Vec::new(),
            message: "Type a web search after @web",
        };
    }
    const HEX: &[u8] = b"0123456789ABCDEF";
    let mut url = String::with_capacity(40 + payload.len() * 3);
    url.push_str("https://www.google.com/search?q=");
    for byte in payload.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            url.push(byte as char);
        } else {
            url.push('%');
            url.push(HEX[(byte >> 4) as usize] as char);
            url.push(HEX[(byte & 15) as usize] as char);
        }
    }
    SearchBatch {
        results: vec![SearchResult {
            kind: ResultKind::Web,
            id: "web".into(),
            title: format!("Search the web for {payload}").into(),
            description: "Open in your default browser".into(),
            action: Action::OpenUrl(url.into()),
        }],
        message: "Enter to search in your browser",
    }
}

fn update_results(payload: &str) -> SearchBatch {
    if !payload.is_empty() {
        return SearchBatch {
            results: Vec::new(),
            message: "Use @update without arguments",
        };
    }
    SearchBatch {
        results: vec![SearchResult {
            kind: ResultKind::System,
            id: "core:update".into(),
            title: "Core updates".into(),
            description: "Check update status or restart to install".into(),
            action: Action::Update,
        }],
        message: "Enter to check for updates",
    }
}

fn command_hints(prefix: &str) -> SearchBatch {
    let normalized = prefix.to_ascii_lowercase();
    let results = [
        ("app", "Search applications"),
        ("calc", "Calculate locally"),
        ("time", "Convert time zones"),
        ("web", "Search the web"),
        ("power", "Power off, restart or sleep"),
        ("quicklink", "Open saved websites, files and folders"),
        ("taskbar", "Show the Windows taskbar"),
        (
            "run",
            "Open a program, folder or URI like Win+R · / runs commands",
        ),
        ("update", "Check Core updates or restart to install"),
        ("media", "Play, pause or skip music · also @music"),
        ("volume", "Change each program's volume · also @mix"),
        (
            "song",
            "Search Spotify songs · optional connection in Music settings",
        ),
        ("album", "Play an album from Spotify"),
        ("artist", "Play an artist from Spotify"),
        ("playlist", "Play one of your Spotify playlists"),
        ("info", "Core's version and the latest release"),
    ]
    .into_iter()
    .filter(|(command, _)| command.starts_with(&normalized))
    .take(VISIBLE_RESULT_LIMIT)
    .map(|(command, description)| SearchResult {
        kind: ResultKind::Command,
        id: command.into(),
        title: format!("@{command}").into(),
        description: description.into(),
        action: Action::FillQuery(format!("@{command} ").into()),
    })
    .collect();
    SearchBatch {
        results,
        message: "Choose a command",
    }
}

#[cfg(test)]
mod recent_tests {
    use super::*;
    use crate::applications::Application;

    #[test]
    fn update_is_an_explicit_command_and_accepts_no_payload() {
        let mut engine = SearchEngine::default();
        let catalog = ApplicationCatalog::default();
        for query in ["@update", "@UPDATE "] {
            assert_eq!(
                engine.search(query, &catalog).results[0].action,
                Action::Update
            );
        }
        assert!(engine
            .search("@update run.exe", &catalog)
            .results
            .is_empty());
        assert_eq!(
            engine.search("@u", &catalog).results[0].title.as_ref(),
            "@update"
        );
        assert_eq!(
            engine.search("@", &catalog).results.len(),
            VISIBLE_RESULT_LIMIT
        );
    }

    #[test]
    fn an_empty_query_shows_recent_apps_and_typing_searches_as_before() {
        let catalog = ApplicationCatalog::new(
            ["Calculator", "Notepad"]
                .into_iter()
                .map(|name| Application {
                    id: format!("id:{name}").into(),
                    name: name.into(),
                    description: "Programs".into(),
                    pinned: false,
                    launches: 0,
                    aliases: Default::default(),
                })
                .collect(),
        );
        let mut engine = SearchEngine::default();
        assert_eq!(
            engine.search("", &catalog).results[0].kind,
            ResultKind::Application
        );
        engine.set_recent_applications(Arc::from([Arc::from("id:Notepad")]));
        let empty = engine.search("  ", &catalog);
        assert_eq!(empty.results.len(), 1);
        assert_eq!(empty.results[0].kind, ResultKind::Recent);
        assert_eq!(&*empty.results[0].title, "Notepad");
        let typed = engine.search("calc", &catalog);
        assert_eq!(typed.results[0].kind, ResultKind::Application);
    }

    #[test]
    fn app_names_skip_converters_and_reach_app_search() {
        let catalog = ApplicationCatalog::new(vec![Application {
            id: "id:code".into(),
            name: "Visual Studio Code".into(),
            description: "Programs".into(),
            pinned: false,
            launches: 0,
            aliases: Default::default(),
        }]);
        let mut engine = SearchEngine::default();
        for query in ["code", "vsc", "visual studio code"] {
            assert!(engine.try_calculation(query).is_none(), "{query}");
            let batch = engine.search(query, &catalog);
            assert_eq!(batch.results[0].kind, ResultKind::Application, "{query}");
            assert_eq!(&*batch.results[0].title, "Visual Studio Code", "{query}");
        }
        let dash = engine.search("-", &catalog);
        assert!(dash.results.is_empty());
        assert_eq!(dash.message, "Finish the expression");
        assert_eq!(
            engine.search(".5 kg to lb", &catalog).results[0]
                .title
                .as_ref(),
            "1.10231131092 lb"
        );
    }

    #[test]
    fn empty_scope_results_follow_the_catalog_they_came_from() {
        let catalog_of = |names: &[&str]| {
            ApplicationCatalog::new(
                names
                    .iter()
                    .map(|name| Application {
                        id: format!("id:{name}").into(),
                        name: (*name).into(),
                        description: "Programs".into(),
                        pinned: false,
                        launches: 0,
                        aliases: Default::default(),
                    })
                    .collect(),
            )
        };
        let titles = |batch: SearchBatch| -> Vec<String> {
            batch
                .results
                .iter()
                .map(|result| result.title.to_string())
                .collect()
        };
        let mut engine = SearchEngine::default();
        let mut catalog = catalog_of(&["Calculator", "Notepad"]);
        for query in ["", "@app ", ""] {
            assert_eq!(
                titles(engine.search(query, &catalog)),
                ["Calculator", "Notepad"],
                "{query:?}"
            );
        }
        // The search worker replaces its catalogs in place, so the same address must not
        // bring back the previous catalog's list.
        catalog = catalog_of(&["Paint"]);
        assert_eq!(titles(engine.search("", &catalog)), ["Paint"]);
        assert_eq!(titles(engine.search("@app ", &catalog)), ["Paint"]);
        // A cancelled empty search is not remembered as an empty catalog.
        let fresh = catalog_of(&["Word"]);
        assert!(engine
            .search_with_cancel("", &fresh, &|| true)
            .results
            .is_empty());
        assert_eq!(titles(engine.search("", &fresh)), ["Word"]);
        // Quicklinks keep their own list next to the application catalogs.
        let quicklinks = catalog_of(&["Docs"]);
        let batch = engine.search_catalogs(">", &catalog, &catalog, &quicklinks, &|| false);
        assert_eq!(titles(batch), ["Docs"]);
        assert_eq!(titles(engine.search("", &catalog)), ["Paint"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn web_search_encodes_reserved_characters_and_utf8() {
        let batch =
            SearchEngine::default().search("@web café & windows", &ApplicationCatalog::default());
        assert_eq!(
            batch.results[0].action,
            Action::OpenUrl("https://www.google.com/search?q=caf%C3%A9%20%26%20windows".into())
        );
    }
    #[test]
    fn bare_constants_remain_searches_but_explicit_calculator_evaluates_them() {
        let catalog = ApplicationCatalog::default();
        let mut engine = SearchEngine::default();
        for query in ["e", "E", "pi", "tau"] {
            assert!(engine.try_calculation(query).is_none(), "{query}");
        }
        let result = engine.search("@calc pi", &catalog);
        assert_eq!(result.results[0].kind, ResultKind::Calculator);
        assert_eq!(
            result.results[0].action,
            Action::CopyText(format_number(std::f64::consts::PI).into())
        );
    }
    #[test]
    fn incomplete_expressions_and_hints_never_create_launch_actions() {
        let catalog = ApplicationCatalog::default();
        let mut engine = SearchEngine::default();
        assert!(engine.search("@calc 2+", &catalog).results.is_empty());
        assert!(matches!(
            engine.search("@cal", &catalog).results[0].action,
            Action::FillQuery(_)
        ));
        assert_eq!(
            engine.search("2+2", &catalog).results[0].action,
            Action::CopyText("4".into())
        );
    }
    #[test]
    fn conversions_route_before_time_zones_and_arithmetic() {
        let catalog = ApplicationCatalog::default();
        let mut engine = SearchEngine::default();
        // `pt` is both US pints and Pacific time: a unit destination decides.
        let pints = engine.search("9 pt to cup", &catalog);
        assert_eq!(pints.results[0].kind, ResultKind::Conversion);
        assert_eq!(pints.results[0].action, Action::CopyText("18".into()));
        let time = engine.search("9 pt to et", &catalog);
        assert!(time
            .results
            .iter()
            .all(|result| result.kind != ResultKind::Conversion));
        // Multi-answer conversions become one copyable row each.
        let colours = engine.search("#ff8800", &catalog);
        assert_eq!(colours.results.len(), 3);
        assert_eq!(
            colours.results[2].action,
            Action::CopyText("#FF8800".into())
        );
        assert_eq!(
            engine.search("2 + 2", &catalog).results[0].action,
            Action::CopyText("4".into())
        );
    }

    #[test]
    fn currency_needs_rates_and_uses_the_latest_snapshot() {
        let catalog = ApplicationCatalog::default();
        let mut engine = SearchEngine::default();
        let unavailable = engine.search("110 usd to eur", &catalog);
        assert!(unavailable.results.is_empty());
        assert!(unavailable.message.contains("Exchange rates"));
        let rates =
            crate::conversions::ExchangeRates::from_ecb_xml(crate::conversions::SAMPLE_ECB_XML)
                .unwrap();
        engine.set_exchange_rates(Some(Arc::new(rates)));
        assert_eq!(
            engine.search("110 usd to eur", &catalog).results[0].action,
            Action::CopyText("100.00".into())
        );
    }

    #[test]
    fn taskbar_keywords_and_commands_offer_the_taskbar_first() {
        let catalog = ApplicationCatalog::default();
        let mut engine = SearchEngine::default();
        for query in ["tb", "Taskbar", "@tb", "@taskbar"] {
            let batch = engine.search(query, &catalog);
            assert_eq!(batch.results[0].action, Action::RevealTaskbar, "{query}");
        }
        assert!(engine
            .search("tbx", &catalog)
            .results
            .iter()
            .all(|result| result.action != Action::RevealTaskbar));
        assert!(engine
            .search("@ta", &catalog)
            .results
            .iter()
            .any(|result| result.title.as_ref() == "@taskbar"));
    }
}
