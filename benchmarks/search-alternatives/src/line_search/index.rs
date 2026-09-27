use super::corpus::{Corpus, Span};
use std::collections::HashMap;

pub const BLOCK_LINES: usize = 64;
pub type Postings = HashMap<u32, Box<[u32]>>;

#[derive(Clone, Copy)]
pub enum Granularity {
    File,
    Block,
    Line,
}

pub struct UnitIndex {
    pub units: Vec<Span>,
    pub postings: Postings,
}

pub fn gram(bytes: &[u8]) -> u32 {
    (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2])
}

fn units(corpus: &Corpus, granularity: Granularity) -> Vec<Span> {
    match granularity {
        Granularity::File => corpus.files.clone(),
        Granularity::Line => (0..corpus.lines.len())
            .map(|first| Span {
                first,
                end: first + 1,
            })
            .collect(),
        Granularity::Block => corpus
            .files
            .iter()
            .flat_map(|file| {
                (file.first..file.end)
                    .step_by(BLOCK_LINES)
                    .map(|first| Span {
                        first,
                        end: (first + BLOCK_LINES).min(file.end),
                    })
            })
            .collect(),
    }
}

impl UnitIndex {
    pub fn build(corpus: &Corpus, granularity: Granularity) -> Self {
        let units = units(corpus, granularity);
        let mut postings: HashMap<u32, Vec<u32>> = HashMap::new();
        for (identifier, unit) in units.iter().enumerate() {
            for line in unit.first..unit.end {
                for bytes in corpus.line(line).windows(3) {
                    let list = postings.entry(gram(bytes)).or_default();
                    if list.last() != Some(&(identifier as u32)) {
                        list.push(identifier as u32);
                    }
                }
            }
        }
        Self {
            // A line identifier already identifies its one-line unit.
            units: if matches!(granularity, Granularity::Line) {
                Vec::new()
            } else {
                units
            },
            postings: postings
                .into_iter()
                .map(|(gram, list)| (gram, list.into_boxed_slice()))
                .collect(),
        }
    }
}

pub fn build_positions(corpus: &Corpus) -> Postings {
    assert!(
        corpus.bytes.len() < u32::MAX as usize,
        "prototype uses bounded 32-bit offsets"
    );
    let mut postings: HashMap<u32, Vec<u32>> = HashMap::new();
    for line in &corpus.lines {
        for (offset, bytes) in corpus.bytes[line.first..line.end].windows(3).enumerate() {
            postings
                .entry(gram(bytes))
                .or_default()
                .push((line.first + offset) as u32);
        }
    }
    postings
        .into_iter()
        .map(|(gram, list)| (gram, list.into_boxed_slice()))
        .collect()
}

pub enum Candidates<'index> {
    Scan,
    Missing,
    Lists(RareLists<'index>),
}

pub struct RareLists<'index> {
    pub first: (&'index [u32], usize),
    pub second: Option<(&'index [u32], usize)>,
}

pub fn rare_lists<'index>(query: &[u8], index: &'index Postings) -> Candidates<'index> {
    if query.len() < 3 {
        return Candidates::Scan;
    }
    let mut first: Option<(&[u32], usize)> = None;
    let mut second: Option<(&[u32], usize)> = None;
    for (offset, bytes) in query.windows(3).enumerate() {
        let Some(list) = index.get(&gram(bytes)) else {
            return Candidates::Missing;
        };
        let candidate = (list.as_ref(), offset);
        if first.is_none_or(|entry| list.len() < entry.0.len()) {
            second = first;
            first = Some(candidate);
        } else if second.is_none_or(|entry| list.len() < entry.0.len()) {
            second = Some(candidate);
        }
    }
    Candidates::Lists(RareLists {
        first: first.expect("a query of three bytes has a gram"),
        second,
    })
}

pub fn intersect(left: &[u32], right: &[u32], output: &mut Vec<u32>) {
    output.clear();
    let (mut left_cursor, mut right_cursor) = (0, 0);
    while left_cursor < left.len() && right_cursor < right.len() {
        match left[left_cursor].cmp(&right[right_cursor]) {
            std::cmp::Ordering::Less => left_cursor += 1,
            std::cmp::Ordering::Greater => right_cursor += 1,
            std::cmp::Ordering::Equal => {
                output.push(left[left_cursor]);
                left_cursor += 1;
                right_cursor += 1;
            }
        }
    }
}
