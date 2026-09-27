use crate::arena::FileEntry;
use crate::index::{pack_trigram, SearchIndex};
use std::collections::HashMap;

pub type CompactHash = HashMap<u32, Box<[u32]>>;

pub fn compact_hash(files: &[FileEntry]) -> CompactHash {
    let mut postings: HashMap<u32, Vec<u32>> = HashMap::new();
    for (identifier, file) in files.iter().enumerate() {
        for trigram in file.name_lower.as_bytes().windows(3) {
            let list = postings.entry(pack_trigram(trigram)).or_default();
            if list.last() != Some(&(identifier as u32)) {
                list.push(identifier as u32);
            }
        }
    }
    postings
        .into_iter()
        .map(|(key, list)| (key, list.into_boxed_slice()))
        .collect()
}

pub struct FlatPostings {
    keys: Vec<u32>,
    offsets: Vec<usize>,
    identifiers: Vec<u32>,
}

impl FlatPostings {
    pub fn from_sorted_pairs(files: &[FileEntry]) -> Self {
        let capacity = files
            .iter()
            .map(|file| file.name_lower.len().saturating_sub(2))
            .sum();
        let mut pairs = Vec::with_capacity(capacity);
        for (identifier, file) in files.iter().enumerate() {
            pairs.extend(
                file.name_lower
                    .as_bytes()
                    .windows(3)
                    .map(|trigram| (pack_trigram(trigram), identifier as u32)),
            );
        }
        pairs.sort_unstable();
        pairs.dedup();
        let mut result = Self {
            keys: Vec::new(),
            offsets: Vec::new(),
            identifiers: Vec::with_capacity(pairs.len()),
        };
        for (key, identifier) in pairs {
            if result.keys.last() != Some(&key) {
                result.keys.push(key);
                result.offsets.push(result.identifiers.len());
            }
            result.identifiers.push(identifier);
        }
        result.offsets.push(result.identifiers.len());
        result.keys.shrink_to_fit();
        result.offsets.shrink_to_fit();
        result
    }

    pub fn get(&self, key: u32) -> Option<&[u32]> {
        let position = self.keys.binary_search(&key).ok()?;
        Some(&self.identifiers[self.offsets[position]..self.offsets[position + 1]])
    }
}

/// None means a short query needs scanning; Some(empty) proves no possible match.
pub fn shortest<'index>(
    query: &str,
    mut lookup: impl FnMut(u32) -> Option<&'index [u32]>,
) -> Option<&'index [u32]> {
    if query.len() < 3 {
        return None;
    }
    let mut shortest: Option<&[u32]> = None;
    for trigram in query.as_bytes().windows(3) {
        let Some(list) = lookup(pack_trigram(trigram)) else {
            return Some(&[]);
        };
        if shortest.is_none_or(|current| list.len() < current.len()) {
            shortest = Some(list);
        }
    }
    shortest
}

pub fn shortest_hash<'index>(query: &str, index: &'index SearchIndex) -> Option<&'index [u32]> {
    shortest(query, |key| {
        index.trigram_postings.get(&key).map(Vec::as_slice)
    })
}

/// Intersect at most three rare postings into reusable storage; verify substrings afterwards.
pub fn intersect_three(
    query: &str,
    index: &SearchIndex,
    output: &mut Vec<u32>,
    temporary: &mut Vec<u32>,
) -> bool {
    output.clear();
    if query.len() < 3 {
        return false;
    }
    let mut selected: [Option<&[u32]>; 3] = [None; 3];
    for trigram in query.as_bytes().windows(3) {
        let Some(list) = index.trigram_postings.get(&pack_trigram(trigram)) else {
            return true;
        };
        if selected
            .iter()
            .flatten()
            .any(|entry| std::ptr::eq(entry.as_ptr(), list.as_ptr()))
        {
            continue;
        }
        let position = selected
            .iter()
            .position(|entry| entry.is_none_or(|entry| list.len() < entry.len()));
        if let Some(position) = position {
            selected.copy_within(position..2, position + 1);
            selected[position] = Some(list);
        }
    }
    let Some(first) = selected[0] else {
        return true;
    };
    output.extend_from_slice(first);
    for next in selected.into_iter().skip(1).flatten() {
        temporary.clear();
        let mut left = 0;
        let mut right = 0;
        while left < output.len() && right < next.len() {
            match output[left].cmp(&next[right]) {
                std::cmp::Ordering::Less => left += 1,
                std::cmp::Ordering::Greater => right += 1,
                std::cmp::Ordering::Equal => {
                    temporary.push(output[left]);
                    left += 1;
                    right += 1;
                }
            }
        }
        std::mem::swap(output, temporary);
    }
    true
}

pub fn byte_mask(text: &str) -> u64 {
    text.bytes()
        .fold(0, |mask, byte| mask | (1_u64 << (byte % 64)))
}
