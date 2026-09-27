use super::corpus::{Corpus, Span};
use super::index::{self, Candidates, Granularity, Postings, UnitIndex};
use memchr::memmem::Finder;

pub struct Indexes {
    pub files: UnitIndex,
    pub blocks: UnitIndex,
    pub lines: UnitIndex,
    pub positions: Postings,
}
impl Indexes {
    pub fn build(corpus: &Corpus) -> Self {
        Self {
            files: UnitIndex::build(corpus, Granularity::File),
            blocks: UnitIndex::build(corpus, Granularity::Block),
            lines: UnitIndex::build(corpus, Granularity::Line),
            positions: index::build_positions(corpus),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Method {
    LineScan,
    BufferScan,
    FileRarest,
    BlockRarest,
    LineRarest,
    LineIntersect,
    PositionRarest,
    PositionPair,
    PositionMerge,
}
impl Method {
    pub const ALL: [Self; 9] = [
        Self::LineScan,
        Self::BufferScan,
        Self::FileRarest,
        Self::BlockRarest,
        Self::LineRarest,
        Self::LineIntersect,
        Self::PositionRarest,
        Self::PositionPair,
        Self::PositionMerge,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::LineScan => "line_scan",
            Self::BufferScan => "buffer_scan",
            Self::FileRarest => "file_rarest",
            Self::BlockRarest => "block_rarest",
            Self::LineRarest => "line_rarest",
            Self::LineIntersect => "line_intersect",
            Self::PositionRarest => "position_rarest",
            Self::PositionPair => "position_pair",
            Self::PositionMerge => "position_merge",
        }
    }
}

#[derive(Default)]
pub struct Scratch {
    pub results: Vec<u32>,
    candidates: Vec<u32>,
}

fn line_scan(
    corpus: &Corpus,
    finder: &Finder<'_>,
    range: Span,
    limit: usize,
    results: &mut Vec<u32>,
) {
    for identifier in range.first..range.end {
        if finder.find(corpus.line(identifier)).is_some() {
            results.push(identifier as u32);
        }
        if results.len() == limit {
            break;
        }
    }
}

fn buffer_scan(
    corpus: &Corpus,
    finder: &Finder<'_>,
    range: Span,
    limit: usize,
    results: &mut Vec<u32>,
) {
    if range.first == range.end {
        return;
    }
    let mut cursor = corpus.lines[range.first].first;
    let end = corpus.lines[range.end - 1].end;
    while cursor < end {
        let Some(relative) = finder.find(&corpus.bytes[cursor..end]) else {
            break;
        };
        let absolute = cursor + relative;
        let identifier = corpus.lines[range.first..range.end]
            .partition_point(|line| line.end <= absolute)
            + range.first;
        results.push(identifier as u32);
        if results.len() == limit {
            break;
        }
        cursor = corpus.lines[identifier].end + 1;
    }
}

fn unit_search(
    corpus: &Corpus,
    index: &UnitIndex,
    query: &[u8],
    method: Method,
    limit: usize,
    scratch: &mut Scratch,
) {
    let finder = Finder::new(query);
    let candidates = index::rare_lists(query, &index.postings);
    let lists = match candidates {
        Candidates::Missing => return,
        Candidates::Scan => {
            buffer_scan(
                corpus,
                &finder,
                Span {
                    first: 0,
                    end: corpus.lines.len(),
                },
                limit,
                &mut scratch.results,
            );
            return;
        }
        Candidates::Lists(lists) => lists,
    };
    let identifiers = if let Some(second) = lists
        .second
        .filter(|_| matches!(method, Method::LineIntersect))
    {
        index::intersect(lists.first.0, second.0, &mut scratch.candidates);
        scratch.candidates.as_slice()
    } else {
        lists.first.0
    };
    for &identifier in identifiers {
        if matches!(method, Method::LineRarest | Method::LineIntersect) {
            let unit = Span {
                first: identifier as usize,
                end: identifier as usize + 1,
            };
            line_scan(corpus, &finder, unit, limit, &mut scratch.results);
        } else {
            let unit = index.units[identifier as usize];
            buffer_scan(corpus, &finder, unit, limit, &mut scratch.results);
        }
        if scratch.results.len() == limit {
            break;
        }
    }
}

fn positional_search(
    corpus: &Corpus,
    positions: &Postings,
    query: &[u8],
    method: Method,
    limit: usize,
    scratch: &mut Scratch,
) {
    let lists = match index::rare_lists(query, positions) {
        Candidates::Missing => return,
        Candidates::Scan => {
            buffer_scan(
                corpus,
                &Finder::new(query),
                Span {
                    first: 0,
                    end: corpus.lines.len(),
                },
                limit,
                &mut scratch.results,
            );
            return;
        }
        Candidates::Lists(lists) => lists,
    };
    let (anchor, query_offset) = lists.first;
    let mut second_cursor = 0;
    for &offset in anchor {
        let Some(start) = (offset as usize).checked_sub(query_offset) else {
            continue;
        };
        if !confirm_second_position(lists.second, start, method, &mut second_cursor) {
            continue;
        }
        if corpus.bytes.get(start..start + query.len()) != Some(query) {
            continue;
        }
        let identifier = corpus.lines.partition_point(|line| line.end <= start) as u32;
        if scratch.results.last() != Some(&identifier) {
            scratch.results.push(identifier);
        }
        if scratch.results.len() == limit {
            break;
        }
    }
}

fn confirm_second_position(
    second: Option<(&[u32], usize)>,
    start: usize,
    method: Method,
    cursor: &mut usize,
) -> bool {
    let Some((offsets, query_offset)) = second else {
        return true;
    };
    let expected = (start + query_offset) as u32;
    match method {
        Method::PositionPair => offsets.binary_search(&expected).is_ok(),
        Method::PositionMerge => {
            while *cursor < offsets.len() && offsets[*cursor] < expected {
                *cursor += 1;
            }
            offsets.get(*cursor) == Some(&expected)
        }
        _ => true,
    }
}

/// Byte-exact, case-sensitive, single-line literals. One ordered result per matching line.
pub fn search(
    corpus: &Corpus,
    indexes: &Indexes,
    query: &[u8],
    method: Method,
    limit: usize,
    scratch: &mut Scratch,
) {
    scratch.results.clear();
    if query.is_empty() || query.contains(&b'\n') || query.contains(&b'\r') || limit == 0 {
        return;
    }
    let range = Span {
        first: 0,
        end: corpus.lines.len(),
    };
    match method {
        Method::LineScan => line_scan(
            corpus,
            &Finder::new(query),
            range,
            limit,
            &mut scratch.results,
        ),
        Method::BufferScan => buffer_scan(
            corpus,
            &Finder::new(query),
            range,
            limit,
            &mut scratch.results,
        ),
        Method::FileRarest => unit_search(corpus, &indexes.files, query, method, limit, scratch),
        Method::BlockRarest => unit_search(corpus, &indexes.blocks, query, method, limit, scratch),
        Method::LineRarest | Method::LineIntersect => {
            unit_search(corpus, &indexes.lines, query, method, limit, scratch)
        }
        Method::PositionRarest | Method::PositionPair | Method::PositionMerge => {
            positional_search(corpus, &indexes.positions, query, method, limit, scratch)
        }
    }
}
