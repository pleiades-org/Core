use crate::arena::IndexArena;
use crate::index::CandidateSet;
use crate::postings::{self, CompactHash, FlatPostings};
use memchr::memmem::Finder;
use std::collections::BinaryHeap;

pub const LIMIT: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Scan,
    Mask,
    Finder,
    OriginalCandidates,
    Rarest,
    Intersect,
    Flat,
    CompactHash,
    PrefixRarest,
}
impl Method {
    pub const ALL: [Self; 9] = [
        Self::Scan,
        Self::Mask,
        Self::Finder,
        Self::OriginalCandidates,
        Self::Rarest,
        Self::Intersect,
        Self::Flat,
        Self::CompactHash,
        Self::PrefixRarest,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Mask => "mask_scan",
            Self::Finder => "memmem_scan",
            Self::OriginalCandidates => "upstream_candidates",
            Self::Rarest => "rarest_posting",
            Self::Intersect => "intersect_three",
            Self::Flat => "flat_postings",
            Self::CompactHash => "compact_hash",
            Self::PrefixRarest => "prefix_then_rarest",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Selector {
    Partial,
    Heap,
    Fixed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Ranked {
    pub tier: u8,
    pub lexical: u32,
    pub identifier: u32,
}

pub struct Catalog {
    pub arena: IndexArena,
    pub lexical: Vec<u32>,
    pub lexical_order: Vec<u32>,
    pub masks: Vec<u64>,
    pub flat: FlatPostings,
    pub compact: CompactHash,
}

impl Catalog {
    pub fn new(arena: IndexArena) -> Self {
        let lexical = crate::corpus::lexical_ranks(&arena);
        let mut lexical_order = vec![0; lexical.len()];
        for (identifier, &rank) in lexical.iter().enumerate() {
            lexical_order[rank as usize] = identifier as u32;
        }
        let masks = arena
            .files
            .iter()
            .map(|file| postings::byte_mask(&file.name_lower))
            .collect();
        let flat = FlatPostings::from_sorted_pairs(&arena.files);
        let compact = postings::compact_hash(&arena.files);
        Self {
            arena,
            lexical,
            lexical_order,
            masks,
            flat,
            compact,
        }
    }
}

#[derive(Default)]
pub struct Scratch {
    pub ranked: Vec<Ranked>,
    candidates: Vec<u32>,
    temporary: Vec<u32>,
    heap: BinaryHeap<Ranked>,
}

fn retain(
    candidate: Ranked,
    output: &mut Vec<Ranked>,
    heap: &mut BinaryHeap<Ranked>,
    selector: Selector,
) {
    match selector {
        Selector::Partial => output.push(candidate),
        Selector::Heap => {
            if heap.len() < LIMIT {
                heap.push(candidate);
            } else if candidate < *heap.peek().expect("heap is full") {
                *heap.peek_mut().expect("heap is full") = candidate;
            }
        }
        Selector::Fixed => {
            let position = output.partition_point(|existing| existing < &candidate);
            if position < LIMIT {
                if output.len() == LIMIT {
                    output.pop();
                }
                output.insert(position, candidate);
            }
        }
    }
}

fn finish(scratch: &mut Scratch, selector: Selector) {
    if matches!(selector, Selector::Heap) {
        scratch.ranked.extend(scratch.heap.drain());
    }
    if scratch.ranked.len() > LIMIT {
        scratch.ranked.select_nth_unstable(LIMIT - 1);
        scratch.ranked.truncate(LIMIT);
    }
    scratch.ranked.sort_unstable();
}

/// Equal result contract for every method: substring membership, exact/prefix/contains tiers,
/// then normalized filename and stable identifier. Caller supplies normalized nonempty input.
pub fn search(
    catalog: &Catalog,
    query: &str,
    method: Method,
    selector: Selector,
    scratch: &mut Scratch,
) {
    scratch.ranked.clear();
    scratch.heap.clear();
    if query.is_empty() {
        return;
    }
    if method == Method::PrefixRarest && fill_prefix_results(catalog, query, scratch) {
        return;
    }
    scratch.ranked.clear();
    let query_mask = postings::byte_mask(query);
    let finder = Finder::new(query.as_bytes());
    let mut evaluate = |identifier: u32| {
        let name = &catalog.arena.files[identifier as usize].name_lower;
        if method == Method::Mask && catalog.masks[identifier as usize] & query_mask != query_mask {
            return;
        }
        let matched = if method == Method::Finder {
            finder.find(name.as_bytes()).is_some()
        } else {
            name.contains(query)
        };
        if !matched {
            return;
        }
        let tier = if name == query {
            0
        } else if name.starts_with(query) {
            1
        } else {
            2
        };
        retain(
            Ranked {
                tier,
                lexical: catalog.lexical[identifier as usize],
                identifier,
            },
            &mut scratch.ranked,
            &mut scratch.heap,
            selector,
        );
    };
    match method {
        Method::OriginalCandidates => match catalog.arena.search.candidates_for(query) {
            CandidateSet::Filtered(identifiers) => {
                identifiers.iter().copied().for_each(&mut evaluate)
            }
            CandidateSet::MiddleOut(identifiers) => {
                identifiers.iter().copied().for_each(&mut evaluate)
            }
        },
        Method::Intersect => {
            if postings::intersect_three(
                query,
                &catalog.arena.search,
                &mut scratch.candidates,
                &mut scratch.temporary,
            ) {
                scratch.candidates.iter().copied().for_each(&mut evaluate);
            } else {
                (0..catalog.arena.files.len() as u32).for_each(&mut evaluate);
            }
        }
        _ => {
            let candidates = match method {
                Method::Rarest | Method::PrefixRarest => {
                    postings::shortest_hash(query, &catalog.arena.search)
                }
                Method::Flat => postings::shortest(query, |key| catalog.flat.get(key)),
                Method::CompactHash => {
                    postings::shortest(query, |key| catalog.compact.get(&key).map(AsRef::as_ref))
                }
                _ => None,
            };
            if let Some(candidates) = candidates {
                candidates.iter().copied().for_each(&mut evaluate);
            } else {
                (0..catalog.arena.files.len() as u32).for_each(&mut evaluate);
            }
        }
    }
    finish(scratch, selector);
}

/// Safe early exit only for the exact/prefix/substring tiers and alphabetical tie-breaks.
/// Usage-based ordering inside a tier invalidates this shortcut unless separately indexed.
fn fill_prefix_results(catalog: &Catalog, query: &str, scratch: &mut Scratch) -> bool {
    let start = catalog.lexical_order.partition_point(|identifier| {
        catalog.arena.files[*identifier as usize]
            .name_lower
            .as_str()
            < query
    });
    for &identifier in catalog.lexical_order[start..].iter().take(LIMIT) {
        let name = &catalog.arena.files[identifier as usize].name_lower;
        if !name.starts_with(query) {
            break;
        }
        scratch.ranked.push(Ranked {
            tier: u8::from(name != query),
            lexical: catalog.lexical[identifier as usize],
            identifier,
        });
    }
    scratch.ranked.len() == LIMIT
}

/// The exact old application's score tiers, applied to this synthetic name corpus.
pub fn legacy_score(name: &str, query: &str) -> Option<u8> {
    if name == query {
        return Some(95);
    }
    if name.starts_with(query) {
        return Some(88);
    }
    if name.contains(query) {
        return Some(76);
    }
    query
        .split_whitespace()
        .all(|word| name.contains(word))
        .then_some(64)
}

pub fn legacy_search(catalog: &Catalog, query: &str) -> Vec<u32> {
    crate::legacy_search_text::take_top_scored(
        0..catalog.arena.files.len() as u32,
        |identifier| legacy_score(&catalog.arena.files[*identifier as usize].name_lower, query),
        LIMIT,
        |identifier| catalog.arena.files[*identifier as usize].name_lower.clone(),
    )
    .into_iter()
    .map(|(_, identifier)| identifier)
    .collect()
}

pub fn borrowed_core_search(catalog: &Catalog, query: &str, indexed: bool, scratch: &mut Scratch) {
    scratch.ranked.clear();
    if query.is_empty() {
        return;
    }
    // Every token is required; any token's rarest posting is a safe superset.
    let candidates = if indexed {
        query
            .split_whitespace()
            .filter_map(|word| postings::shortest_hash(word, &catalog.arena.search))
            .min_by_key(|list| list.len())
    } else {
        None
    };
    let mut evaluate = |identifier: u32| {
        if let Some(score) =
            legacy_score(&catalog.arena.files[identifier as usize].name_lower, query)
        {
            scratch.ranked.push(Ranked {
                tier: 255 - score,
                lexical: catalog.lexical[identifier as usize],
                identifier,
            });
        }
    };
    if let Some(candidates) = candidates {
        candidates.iter().copied().for_each(&mut evaluate);
    } else {
        (0..catalog.arena.files.len() as u32).for_each(&mut evaluate);
    }
    finish(scratch, Selector::Partial);
}

/// Retains ALL previous matches. Reusing only the displayed top 12 would lose results.
#[derive(Default)]
pub struct Incremental {
    previous: String,
    matches: Vec<u32>,
}
impl Incremental {
    pub fn update(&mut self, arena: &IndexArena, query: &str) -> &[u32] {
        if !self.previous.is_empty() && query.starts_with(&self.previous) {
            self.matches
                .retain(|identifier| arena.files[*identifier as usize].name_lower.contains(query));
        } else {
            self.matches.clear();
            self.matches.extend(
                arena
                    .files
                    .iter()
                    .enumerate()
                    .filter(|(_, file)| file.name_lower.contains(query))
                    .map(|(identifier, _)| identifier as u32),
            );
        }
        self.previous.clear();
        self.previous.push_str(query);
        &self.matches
    }
}
