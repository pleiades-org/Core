use super::candidate_index::CandidateIndex;
use crate::search::{normalize, normalize_into};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

/// Distinguishes catalogs so per-catalog caches never outlive the catalog they describe.
static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1);
/// `SearchScratch::slots` value for an application with no candidate in the current search.
const NO_SLOT: usize = usize::MAX;
/// Launch counts above this rank the same.
const LAUNCH_CAP: u32 = 1000;
/// `Candidate` bit layout, low to high: ordinal, capped launches (10 bits), unpinned, class.
const LAUNCH_SHIFT: u32 = 32;
const UNPINNED_SHIFT: u32 = LAUNCH_SHIFT + 10;
const CLASS_SHIFT: u32 = UNPINNED_SHIFT + 1;

#[derive(Clone, Debug)]
pub struct Application {
    pub id: Arc<str>,
    pub name: Arc<str>,
    pub description: Arc<str>,
    pub pinned: bool,
    pub launches: u32,
    /// Other names Windows knows the app by, such as `cmd` for Command Prompt (the program a
    /// shortcut starts) or `wt` for Windows Terminal (an app execution alias).
    pub aliases: Arc<[Arc<str>]>,
}

#[derive(Debug)]
struct PreparedApplication {
    application: Application,
    normalized_name: String,
    initials: String,
    /// Where each word of the normalized name starts, so searches need not split it again.
    /// Empty for names too long for 16-bit offsets, which are split when searched.
    word_starts: Box<[u16]>,
}

impl PreparedApplication {
    /// Whether a word of the name (a run of letters and digits) starts with `term`. A term with
    /// any other character can never start a word.
    fn has_word_starting_with(&self, term: &str) -> bool {
        let name = &self.normalized_name;
        if !term.chars().all(char::is_alphanumeric) {
            return false;
        }
        if name.len() > usize::from(u16::MAX) {
            return name
                .split(|character: char| !character.is_alphanumeric())
                .any(|word| word.starts_with(term));
        }
        self.word_starts
            .iter()
            .any(|&start| name[usize::from(start)..].starts_with(term))
    }
}

fn word_starts(name: &str) -> Box<[u16]> {
    if name.len() > usize::from(u16::MAX) {
        return Box::default();
    }
    let mut starts = Vec::new();
    let mut in_word = false;
    for (offset, character) in name.char_indices() {
        let alphanumeric = character.is_alphanumeric();
        if alphanumeric && !in_word {
            starts.push(offset as u16);
        }
        in_word = alphanumeric;
    }
    starts.into_boxed_slice()
}

#[derive(Debug, Default)]
pub struct ApplicationCatalog {
    applications: Vec<PreparedApplication>,
    index: Option<CandidateIndex>,
    initials_order: Box<[u32]>,
    /// Normalized alias and the application it belongs to, sorted by alias.
    aliases: Box<[(String, usize)]>,
    /// Identifier to the first ordinal listing it, for resolving recent apps.
    ordinals_by_id: HashMap<Arc<str>, usize>,
    /// Unique per built catalog; `0` only for the empty default catalog.
    identity: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum MatchClass {
    Exact,
    Prefix,
    WordPrefix,
    Acronym,
    Substring,
}

/// The ranking order packed into one integer: match class, then pinned first, then more
/// launches, then catalog order (normalized name, then ID). Catalogs hold far fewer than
/// 2^32 apps, so the ordinal fits the low 32 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Candidate(u64);

impl Candidate {
    fn new(class: MatchClass, pinned: bool, launches: u32, ordinal: usize) -> Self {
        Self(
            (class as u64) << CLASS_SHIFT
                | u64::from(!pinned) << UNPINNED_SHIFT
                | u64::from(LAUNCH_CAP - launches.min(LAUNCH_CAP)) << LAUNCH_SHIFT
                | ordinal as u64,
        )
    }

    fn ordinal(self) -> usize {
        (self.0 & u64::from(u32::MAX)) as usize
    }

    /// Keeps the better of its class and `class`.
    fn improve(&mut self, class: MatchClass) {
        let class = (class as u64).min(self.0 >> CLASS_SHIFT);
        self.0 = self.0 & ((1 << CLASS_SHIFT) - 1) | class << CLASS_SHIFT;
    }
}

#[derive(Default)]
pub struct SearchScratch {
    candidates: Vec<Candidate>,
    /// Ordinal to its index in `candidates`, or `NO_SLOT`; only used slots are reset.
    slots: Vec<usize>,
    normalized_query: String,
}

impl SearchScratch {
    fn reset(&mut self, application_count: usize) {
        for candidate in &self.candidates {
            self.slots[candidate.ordinal()] = NO_SLOT;
        }
        self.candidates.clear();
        if self.slots.len() < application_count {
            self.slots.resize(application_count, NO_SLOT);
        }
    }
}

pub struct RankedApplication<'catalog> {
    pub application: &'catalog Application,
}

impl ApplicationCatalog {
    pub fn entries(&self) -> impl Iterator<Item = &Application> {
        self.applications
            .iter()
            .map(|prepared| &prepared.application)
    }

    pub fn new(applications: Vec<Application>) -> Self {
        let mut applications: Vec<_> = applications
            .into_iter()
            .map(|application| {
                let normalized_name = normalize(&application.name).into_owned();
                let initials = normalized_name
                    .split(|character: char| !character.is_alphanumeric())
                    .filter_map(|word| word.chars().next())
                    .collect();
                PreparedApplication {
                    application,
                    word_starts: word_starts(&normalized_name),
                    normalized_name,
                    initials,
                }
            })
            .collect();
        applications.sort_by(|left, right| {
            left.normalized_name
                .cmp(&right.normalized_name)
                .then_with(|| left.application.id.cmp(&right.application.id))
        });
        let index = CandidateIndex::build(
            applications
                .iter()
                .map(|prepared| prepared.normalized_name.as_str()),
        );
        // The index exists only for catalogs of at most 65,535 names, so ordinals fit.
        let mut initials_order = if index.is_some() {
            (0..applications.len() as u32).collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        initials_order.sort_unstable_by(|&left, &right| {
            applications[left as usize]
                .initials
                .cmp(&applications[right as usize].initials)
                .then(left.cmp(&right))
        });
        let mut aliases: Vec<(String, usize)> = applications
            .iter()
            .enumerate()
            .flat_map(|(ordinal, prepared)| {
                prepared
                    .application
                    .aliases
                    .iter()
                    .map(move |alias| (normalize(alias).into_owned(), ordinal))
            })
            .filter(|(alias, _)| !alias.is_empty())
            .collect();
        aliases.sort_unstable();
        aliases.dedup();
        let mut ordinals_by_id = HashMap::with_capacity(applications.len());
        for (ordinal, prepared) in applications.iter().enumerate() {
            ordinals_by_id
                .entry(prepared.application.id.clone())
                .or_insert(ordinal);
        }
        Self {
            applications,
            index,
            initials_order: initials_order.into_boxed_slice(),
            aliases: aliases.into_boxed_slice(),
            ordinals_by_id,
            identity: NEXT_IDENTITY.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// The first entry with this identifier: a catalog can list the same identifier twice
    /// (a Start Menu shortcut in two folders).
    pub fn find(&self, id: &str) -> Option<&Application> {
        let ordinal = *self.ordinals_by_id.get(id)?;
        Some(&self.applications[ordinal].application)
    }

    /// Equal only for the same built catalog, even if a new one reuses its memory.
    pub(crate) fn identity(&self) -> u64 {
        self.identity
    }

    pub fn len(&self) -> usize {
        self.applications.len()
    }

    pub fn is_empty(&self) -> bool {
        self.applications.is_empty()
    }

    /// Retained posting and initials-order bytes; excludes names and ranking scratch.
    pub fn index_memory_bytes(&self) -> usize {
        self.index.as_ref().map_or(0, CandidateIndex::memory_bytes)
            + std::mem::size_of_val(&*self.initials_order)
    }

    /// Use bounded postings where available, preserving the scan's complete ranking.
    /// Reuse candidate capacity and format only the final selected rows.
    pub fn search<'catalog>(
        &'catalog self,
        query: &str,
        limit: usize,
        scratch: &mut SearchScratch,
    ) -> Vec<RankedApplication<'catalog>> {
        self.search_with_cancel(query, limit, scratch, &|| false)
    }

    pub fn search_with_cancel<'catalog>(
        &'catalog self,
        query: &str,
        limit: usize,
        scratch: &mut SearchScratch,
        cancelled: &impl Fn() -> bool,
    ) -> Vec<RankedApplication<'catalog>> {
        scratch.reset(self.applications.len());
        if limit == 0 {
            return Vec::new();
        }
        // Ranking needs the scratch mutably, so the reused query storage is lent out meanwhile.
        let mut buffer = std::mem::take(&mut scratch.normalized_query);
        let results = self.rank(
            normalize_into(query, &mut buffer),
            limit,
            scratch,
            cancelled,
        );
        scratch.normalized_query = buffer;
        results
    }

    fn rank<'catalog>(
        &'catalog self,
        normalized: &str,
        limit: usize,
        scratch: &mut SearchScratch,
        cancelled: &impl Fn() -> bool,
    ) -> Vec<RankedApplication<'catalog>> {
        let collected = match self
            .index
            .as_ref()
            .map(|index| index.candidates(normalized))
        {
            Some(Some(candidates)) => {
                self.collect_indexed(normalized, candidates, scratch, cancelled)
            }
            // No gram narrows the search: one or two letters, or only very common grams.
            Some(None) if self.collect_prefixed(normalized, limit, scratch, cancelled) => true,
            _ => self.collect_scanned(normalized, scratch, cancelled),
        };
        if !collected {
            return Vec::new();
        }
        self.collect_aliases(normalized, scratch);
        let count = limit.min(scratch.candidates.len());
        if count < scratch.candidates.len() {
            scratch.candidates.select_nth_unstable(count);
        }
        scratch.candidates[..count].sort_unstable();
        scratch.candidates[..count]
            .iter()
            .map(|candidate| RankedApplication {
                application: &self.applications[candidate.ordinal()].application,
            })
            .collect()
    }

    /// Names starting with the query are one range of the sorted catalog, as are aliases, and
    /// match exactly or as a prefix. Every other app ranks as a word prefix or lower, below
    /// all of them, so when they number at least `limit` the rest of the catalog cannot place
    /// and is not scanned. The whole range is still ranked, so pins and launch counts order it.
    /// This differs from the alphabetical early exit that CORE_SEARCH_EXPERIMENTS.md rejects:
    /// that one kept only the first `limit` names of the range, which pins and launches reorder.
    /// False when the scan is still needed, with the scratch left empty for it.
    fn collect_prefixed(
        &self,
        query: &str,
        limit: usize,
        scratch: &mut SearchScratch,
        cancelled: &impl Fn() -> bool,
    ) -> bool {
        let start = self
            .applications
            .partition_point(|prepared| prepared.normalized_name.as_str() < query);
        let length = self.applications[start..]
            .partition_point(|prepared| prepared.normalized_name.starts_with(query));
        for (checked, ordinal) in (start..start + length).enumerate() {
            if checked % 64 == 0 && cancelled() {
                scratch.reset(self.applications.len());
                return false;
            }
            if let Some(class) = match_class(&self.applications[ordinal], query) {
                self.push_candidate(ordinal, class, scratch);
            }
        }
        self.collect_aliases(query, scratch);
        if scratch.candidates.len() >= limit {
            return true;
        }
        scratch.reset(self.applications.len());
        false
    }

    fn collect_scanned(
        &self,
        query: &str,
        scratch: &mut SearchScratch,
        cancelled: &impl Fn() -> bool,
    ) -> bool {
        for (ordinal, prepared) in self.applications.iter().enumerate() {
            if ordinal % 64 == 0 && cancelled() {
                return false;
            }
            if let Some(class) = match_class(prepared, query) {
                self.push_candidate(ordinal, class, scratch);
            }
        }
        true
    }

    fn collect_indexed(
        &self,
        query: &str,
        candidates: &[u16],
        scratch: &mut SearchScratch,
        cancelled: &impl Fn() -> bool,
    ) -> bool {
        if cancelled() {
            return false;
        }
        for (checked, &ordinal) in candidates.iter().enumerate() {
            if checked % 64 == 0 && cancelled() {
                return false;
            }
            if let Some(class) = match_class(&self.applications[usize::from(ordinal)], query) {
                if class != MatchClass::Acronym {
                    self.push_candidate(usize::from(ordinal), class, scratch);
                }
            }
        }
        // Initials are a different match relation: exact query grams must not filter them out.
        let start = self.initials_order.partition_point(|&ordinal| {
            self.applications[ordinal as usize].initials.as_str() < query
        });
        for (checked, &ordinal) in self.initials_order[start..].iter().enumerate() {
            if checked % 64 == 0 && cancelled() {
                return false;
            }
            let prepared = &self.applications[ordinal as usize];
            if !prepared.initials.starts_with(query) {
                break;
            }
            if match_class(prepared, query) == Some(MatchClass::Acronym) {
                self.push_candidate(ordinal as usize, MatchClass::Acronym, scratch);
            }
        }
        true
    }

    /// Typing an alias (`cmd`) finds the app as if its name matched: exactly or as a prefix.
    fn collect_aliases(&self, query: &str, scratch: &mut SearchScratch) {
        if query.is_empty() {
            return;
        }
        let start = self
            .aliases
            .partition_point(|(alias, _)| alias.as_str() < query);
        for (alias, ordinal) in self.aliases[start..].iter() {
            if !alias.starts_with(query) {
                break;
            }
            let class = if alias == query {
                MatchClass::Exact
            } else {
                MatchClass::Prefix
            };
            match scratch.slots[*ordinal] {
                NO_SLOT => self.push_candidate(*ordinal, class, scratch),
                slot => scratch.candidates[slot].improve(class),
            }
        }
    }

    fn push_candidate(&self, ordinal: usize, class: MatchClass, scratch: &mut SearchScratch) {
        let application = &self.applications[ordinal].application;
        scratch.slots[ordinal] = scratch.candidates.len();
        scratch.candidates.push(Candidate::new(
            class,
            application.pinned,
            application.launches,
            ordinal,
        ));
    }
}

fn match_class(prepared: &PreparedApplication, query: &str) -> Option<MatchClass> {
    let name = &prepared.normalized_name;
    if query.is_empty() || name == query {
        return Some(MatchClass::Exact);
    }
    if name.starts_with(query) {
        return Some(MatchClass::Prefix);
    }
    if query
        .split_whitespace()
        .all(|term| prepared.has_word_starting_with(term))
    {
        return Some(MatchClass::WordPrefix);
    }
    if prepared.initials.starts_with(query) {
        return Some(MatchClass::Acronym);
    }
    name.contains(query).then_some(MatchClass::Substring)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Reverse;
    fn application(identifier: &str, name: &str, pinned: bool) -> Application {
        Application {
            id: identifier.into(),
            name: name.into(),
            description: "App".into(),
            pinned,
            launches: 0,
            aliases: Default::default(),
        }
    }
    #[test]
    fn aliases_find_apps_whose_names_do_not_match() {
        let mut command_prompt = application("cmd", "Command Prompt", false);
        command_prompt.aliases = Arc::from([Arc::from("cmd")]);
        let mut terminal = application("wt", "Windows Terminal", false);
        terminal.aliases = Arc::from([Arc::from("wt")]);
        let catalog = ApplicationCatalog::new(vec![
            command_prompt,
            terminal,
            application("cmder", "CMD Tools", false),
        ]);
        let mut scratch = SearchScratch::default();
        let names = |query: &str, scratch: &mut SearchScratch| -> Vec<String> {
            catalog
                .search(query, 8, scratch)
                .iter()
                .map(|ranked| ranked.application.name.to_string())
                .collect()
        };
        // An exact alias ranks like an exact name, ahead of a name that merely starts with it.
        assert_eq!(names("cmd", &mut scratch), ["Command Prompt", "CMD Tools"]);
        assert_eq!(names("WT", &mut scratch), ["Windows Terminal"]);
        // A partly typed alias still finds the app, without duplicating it.
        assert_eq!(
            names("cm", &mut scratch)
                .iter()
                .filter(|name| *name == "Command Prompt")
                .count(),
            1
        );
        assert!(names("xyz", &mut scratch).is_empty());
    }

    #[test]
    fn alias_and_name_matching_one_app_merge_into_its_best_class() {
        let mut editor = application("editor", "Code Editor", false);
        editor.aliases = Arc::from([Arc::from("code"), Arc::from("codeedit")]);
        let catalog = ApplicationCatalog::new(vec![
            application("codex", "Codex Tools", true),
            editor,
            application("other", "Other Code", false),
        ]);
        let mut scratch = SearchScratch::default();
        let mut ids = |query: &str| -> Vec<String> {
            catalog
                .search(query, 8, &mut scratch)
                .iter()
                .map(|ranked| ranked.application.id.to_string())
                .collect()
        };
        // Name prefix plus exact alias: listed once, ranked as exact above the pinned prefix.
        assert_eq!(ids("code"), ["editor", "codex", "other"]);
        // Two aliases starting with the query still give one row.
        assert_eq!(ids("cod"), ["codex", "editor", "other"]);
        assert_eq!(ids("codee"), ["editor"]);
        // Reused scratch starts clean after a larger search and on another catalog.
        let small = ApplicationCatalog::new(vec![application("other", "Other Code", false)]);
        assert_eq!(small.search("code", 8, &mut scratch).len(), 1);
        assert_eq!(catalog.search("code", 8, &mut scratch).len(), 3);
    }

    #[test]
    fn indexed_alias_merging_equals_the_scan() {
        let applications: Vec<_> = (0..300)
            .map(|index| {
                let mut application = application(
                    &format!("app:{index}"),
                    &format!("Code Tool {index}"),
                    index % 7 == 0,
                );
                application.launches = (index % 13) as u32;
                application.aliases = Arc::from([
                    Arc::from(format!("code{index}")),
                    Arc::from(format!("tool{}", index % 5)),
                ]);
                application
            })
            .collect();
        let indexed = ApplicationCatalog::new(applications.clone());
        assert!(indexed.index.is_some());
        let mut scanned = ApplicationCatalog::new(applications);
        scanned.index = None;
        let mut scratch = SearchScratch::default();
        for query in [
            "code",
            "code1",
            "code12",
            "tool",
            "tool3",
            "ct",
            "code tool 1",
        ] {
            for limit in [1, 8, 500] {
                let indexed_ids: Vec<_> = indexed
                    .search(query, limit, &mut scratch)
                    .into_iter()
                    .map(|ranked| ranked.application.id.clone())
                    .collect();
                let scanned_ids: Vec<_> = scanned
                    .search(query, limit, &mut scratch)
                    .into_iter()
                    .map(|ranked| ranked.application.id.clone())
                    .collect();
                assert_eq!(indexed_ids, scanned_ids, "query {query:?}, limit {limit}");
                let mut unique = indexed_ids.clone();
                unique.sort();
                unique.dedup();
                assert_eq!(unique.len(), indexed_ids.len(), "query {query:?}");
            }
        }
    }

    #[test]
    fn exact_matches_beat_pinned_substrings_and_acronyms_are_kept() {
        let catalog = ApplicationCatalog::new(vec![
            application("1", "Visual Studio Code", true),
            application("2", "Code", false),
        ]);
        let mut scratch = SearchScratch::default();
        assert_eq!(
            catalog.search("code", 8, &mut scratch)[0]
                .application
                .id
                .as_ref(),
            "2"
        );
        assert_eq!(
            catalog.search("vsc", 8, &mut scratch)[0]
                .application
                .id
                .as_ref(),
            "1"
        );
        assert_eq!(
            catalog.search("studio co", 8, &mut scratch)[0]
                .application
                .id
                .as_ref(),
            "1"
        );
        assert!(catalog.search("missing", 8, &mut scratch).is_empty());
    }
    #[test]
    fn ties_are_stable_and_equal_display_names_keep_distinct_identifiers() {
        let catalog = ApplicationCatalog::new(vec![
            application("b", "Editor", false),
            application("a", "Editor", false),
        ]);
        let mut scratch = SearchScratch::default();
        assert_eq!(
            catalog.search("", 1, &mut scratch)[0]
                .application
                .id
                .as_ref(),
            "a"
        );
        assert_eq!(catalog.search("editor", 8, &mut scratch).len(), 2);
        assert!(catalog.search("", 0, &mut scratch).is_empty());
    }

    #[test]
    fn cancelled_search_returns_no_partial_results_and_scratch_can_be_reused() {
        use std::cell::Cell;
        let catalog = ApplicationCatalog::new(
            (0..200)
                .map(|index| application(&index.to_string(), "Editor", false))
                .collect(),
        );
        let mut scratch = SearchScratch::default();
        let checks = Cell::new(0);
        let results = catalog.search_with_cancel("editor", 8, &mut scratch, &|| {
            checks.set(checks.get() + 1);
            checks.get() == 2
        });
        assert!(results.is_empty());
        assert_eq!(checks.get(), 2);
        assert_eq!(catalog.search("editor", 8, &mut scratch).len(), 8);
    }

    /// The indexed catalog and a copy that always scans, which is the reference.
    fn indexed_and_scanned(
        applications: Vec<Application>,
    ) -> (ApplicationCatalog, ApplicationCatalog) {
        let indexed = ApplicationCatalog::new(applications.clone());
        assert!(indexed.index.is_some());
        let mut scanned = ApplicationCatalog::new(applications);
        scanned.index = None;
        (indexed, scanned)
    }

    fn assert_same_results(
        indexed: &ApplicationCatalog,
        scanned: &ApplicationCatalog,
        queries: &[String],
        limits: &[usize],
    ) {
        let mut indexed_scratch = SearchScratch::default();
        let mut scanned_scratch = SearchScratch::default();
        for query in queries {
            for &limit in limits {
                let indexed_ids: Vec<_> = indexed
                    .search(query, limit, &mut indexed_scratch)
                    .into_iter()
                    .map(|ranked| ranked.application.id.clone())
                    .collect();
                let scanned_ids: Vec<_> = scanned
                    .search(query, limit, &mut scanned_scratch)
                    .into_iter()
                    .map(|ranked| ranked.application.id.clone())
                    .collect();
                assert_eq!(indexed_ids, scanned_ids, "query {query:?}, limit {limit}");
            }
        }
    }

    #[test]
    fn one_and_two_letter_queries_equal_the_scan() {
        // Many names share a first letter, so the prefix range alone fills the results, while
        // pinned and often launched apps elsewhere match only as word prefixes or substrings.
        let names = [
            "Visual Studio Code",
            "Vim",
            "Code Visual",
            "Windows Terminal",
            "Terminal Preview",
            "Ab",
            "A",
            "Σίσυφος Editor",
            "東京 Calendar",
            "Editor-Portable",
        ];
        let applications: Vec<_> = (0..500)
            .map(|index| {
                let mut application = application(
                    &format!("app:{index}"),
                    &format!("{} {index}", names[index % names.len()]),
                    index % 11 == 0,
                );
                application.launches = (index * 37 % 1500) as u32;
                if index % 25 == 0 {
                    application.aliases = Arc::from([Arc::from(format!("vx{index}"))]);
                }
                application
            })
            .collect();
        let (indexed, scanned) = indexed_and_scanned(applications);
        let mut characters: Vec<char> = names
            .iter()
            .flat_map(|name| name.chars())
            .chain("0123456789 -x".chars())
            .collect();
        characters.sort_unstable();
        characters.dedup();
        let mut queries: Vec<String> = characters.iter().map(char::to_string).collect();
        for first in &characters {
            for second in &characters {
                queries.push(format!("{first}{second}"));
            }
        }
        assert_same_results(&indexed, &scanned, &queries, &[1, 8, 500]);
    }

    #[test]
    fn ten_thousand_apps_keep_an_index_within_budget() {
        let names = [
            "Visual Studio Code",
            "Windows Terminal",
            "Firefox Developer Edition",
            "PowerShell",
            "Σίσυφος Editor",
            "東京 Calendar",
        ];
        let applications: Vec<_> = (0..10_000)
            .map(|index| {
                let mut application = application(
                    &format!("app:{index}"),
                    &format!("{} {index}", names[index % names.len()]),
                    index % 29 == 0,
                );
                application.launches = (index % 97) as u32;
                application
            })
            .collect();
        let (indexed, scanned) = indexed_and_scanned(applications);
        let index_bytes = indexed
            .index
            .as_ref()
            .map_or(0, CandidateIndex::memory_bytes);
        assert!(index_bytes > 0);
        assert!(indexed.index_memory_bytes() <= 512 * 1024);
        let queries: Vec<String> = [
            "",
            "v",
            "vi",
            "vis",
            "visual",
            "code",
            "vsc",
            "terminal 12",
            "ΣΊΣ",
            "東京",
            "9999",
            "not-found",
            "power shell",
        ]
        .map(String::from)
        .into();
        assert_same_results(&indexed, &scanned, &queries, &[1, 8]);
    }

    #[test]
    fn grams_left_out_of_a_full_index_still_find_every_match() {
        // Too many grams for the budget: the most common are left out rather than the index.
        let applications: Vec<_> = (0..10_000)
            .map(|index| {
                application(
                    &format!("app:{index}"),
                    &format!("portableofficesuitepro{index:05}"),
                    index % 13 == 0,
                )
            })
            .collect();
        let (indexed, scanned) = indexed_and_scanned(applications);
        let queries: Vec<String> = [
            "portable",
            "office",
            "suite",
            "pro0",
            "pro01234",
            "tablesuite",
            "ro012",
            "p",
            "po",
            "ps",
            "zz",
        ]
        .map(String::from)
        .into();
        assert_same_results(&indexed, &scanned, &queries, &[1, 8]);
    }

    #[test]
    fn word_starts_match_splitting_the_name() {
        for name in [
            "visual studio code",
            "--editor--portable",
            "σίσυφος editor",
            "東京 calendar",
            "c++ tools",
            "",
        ] {
            let prepared = PreparedApplication {
                application: application("id", name, false),
                normalized_name: name.into(),
                initials: String::new(),
                word_starts: word_starts(name),
            };
            for term in [
                "v", "st", "code", "editor", "port", "σίσ", "東", "cal", "c", "c++", "tools", "x",
                "--e", "e-",
            ] {
                let split = name
                    .split(|character: char| !character.is_alphanumeric())
                    .any(|word| word.starts_with(term));
                assert_eq!(
                    prepared.has_word_starting_with(term),
                    split,
                    "{name:?} {term:?}"
                );
            }
        }
    }

    #[test]
    fn packed_candidates_order_like_their_fields() {
        let fields = [
            (MatchClass::Exact, false, 0, 9),
            (MatchClass::Exact, true, 0, 9),
            (MatchClass::Prefix, true, 5_000, 0),
            (MatchClass::Prefix, true, 1_000, 1),
            (MatchClass::Prefix, true, 999, 0),
            (MatchClass::Substring, true, 0, 0),
            (MatchClass::Prefix, false, 0, u32::MAX as usize),
        ];
        let key = |&(class, pinned, launches, ordinal): &(MatchClass, bool, u32, usize)| {
            (class, !pinned, Reverse(launches.min(LAUNCH_CAP)), ordinal)
        };
        for left in &fields {
            for right in &fields {
                assert_eq!(
                    Candidate::new(left.0, left.1, left.2, left.3)
                        .cmp(&Candidate::new(right.0, right.1, right.2, right.3)),
                    key(left).cmp(&key(right)),
                    "{left:?} {right:?}"
                );
            }
            let mut candidate = Candidate::new(left.0, left.1, left.2, left.3);
            assert_eq!(candidate.ordinal(), left.3);
            candidate.improve(MatchClass::Prefix);
            assert_eq!(
                candidate,
                Candidate::new(left.0.min(MatchClass::Prefix), left.1, left.2, left.3)
            );
        }
    }

    #[test]
    fn indexed_results_equal_the_scan_for_substrings_words_initials_and_unicode() {
        let names = [
            "Visual Studio Code",
            "Windows Terminal",
            "Code Visual",
            "Vsc Tools",
            "Σίσυφος Editor",
            "東京 Calendar",
            "Repeated aaaab",
            "Editor-Portable",
        ];
        let applications: Vec<_> = (0..400)
            .map(|index| {
                let mut application = application(
                    &format!("app:{index}"),
                    &format!("{} {index}", names[index % names.len()]),
                    index % 11 == 0,
                );
                application.launches = (index % 31) as u32;
                application
            })
            .collect();
        let indexed = ApplicationCatalog::new(applications.clone());
        assert!(indexed.index.is_some());
        let mut scanned = ApplicationCatalog::new(applications);
        scanned.index = None;
        let mut queries = vec![
            "",
            "v",
            "vsc",
            "wt",
            "code vis",
            "studio code",
            "editor ΣΊΣ",
            "@missing",
            "東京",
            "aaaa",
            "\tvisual\tstudio ",
        ];
        for name in names {
            for (start, _) in name.char_indices() {
                for end in name
                    .char_indices()
                    .map(|(position, _)| position)
                    .chain(Some(name.len()))
                    .filter(|&end| end > start)
                {
                    queries.push(&name[start..end]);
                }
            }
        }
        let mut indexed_scratch = SearchScratch::default();
        let mut scanned_scratch = SearchScratch::default();
        for query in queries {
            for limit in [1, 8, 500] {
                let indexed_ids: Vec<_> = indexed
                    .search(query, limit, &mut indexed_scratch)
                    .into_iter()
                    .map(|ranked| ranked.application.id.clone())
                    .collect();
                let scanned_ids: Vec<_> = scanned
                    .search(query, limit, &mut scanned_scratch)
                    .into_iter()
                    .map(|ranked| ranked.application.id.clone())
                    .collect();
                assert_eq!(indexed_ids, scanned_ids, "query {query:?}, limit {limit}");
                if query == "editor ΣΊΣ" {
                    assert!(!indexed_ids.is_empty());
                }
            }
        }
    }
}
