//! Separate, dependency-free relevance experiment. It is not a production fuzzy matcher.
use std::cmp::Reverse;
use std::hint::black_box;
use std::time::Instant;

const NAMES: &[&str] = &[
    "Visual Studio Code",
    "Visual Studio",
    "Windows Terminal",
    "Windows Media Player",
    "Google Chrome",
    "Mozilla Firefox",
    "Microsoft Excel",
    "Microsoft Word",
    "Notepad++",
    "Adobe Photoshop",
    "Adobe Acrobat",
    "Café Tools",
    "東京 Maps",
    "Calculator",
    "Calendar",
    "File Explorer",
    "Task Manager",
    "System Settings",
    "Audio Mixer",
    "Video Editor",
    "Password Manager",
    "Git Bash",
    "GitHub Desktop",
    "Spotify",
    "Discord",
    "Steam",
    "Paint",
    "PowerShell",
    "OBS Studio",
    "VLC Media Player",
];

const INTENTS: &[(&str, Option<&str>)] = &[
    ("visual studio code", Some("Visual Studio Code")),
    ("windows term", Some("Windows Terminal")),
    ("firefox", Some("Mozilla Firefox")),
    ("code visual", Some("Visual Studio Code")),
    ("player media windows", Some("Windows Media Player")),
    ("vsc", Some("Visual Studio Code")),
    ("wt", Some("Windows Terminal")),
    ("wmp", Some("Windows Media Player")),
    ("gc", Some("Google Chrome")),
    ("chorme", Some("Google Chrome")),
    ("firefx", Some("Mozilla Firefox")),
    ("terrminal", Some("Windows Terminal")),
    ("excwl", Some("Microsoft Excel")),
    ("photshop", Some("Adobe Photoshop")),
    ("café", Some("Café Tools")),
    ("CAFÉ", Some("Café Tools")),
    ("東京", Some("東京 Maps")),
    ("zzqxy", None),
    ("calcxyz", None),
    ("", None),
];

struct Application {
    display: String,
    normalized: String,
    initials: String,
    words: Vec<String>,
}

impl Application {
    fn new(display: String) -> Self {
        let normalized = display.to_lowercase();
        let words: Vec<_> = normalized
            .split(|character: char| !character.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect();
        let initials = words
            .iter()
            .filter_map(|word| word.chars().next())
            .collect();
        Self {
            display,
            normalized,
            initials,
            words,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    Existing,
    Subsequence,
    InitialsAndTypo,
}
impl Mode {
    const ALL: [Self; 3] = [Self::Existing, Self::Subsequence, Self::InitialsAndTypo];
    fn label(self) -> &'static str {
        match self {
            Self::Existing => "existing",
            Self::Subsequence => "subsequence",
            Self::InitialsAndTypo => "initials_and_typo",
        }
    }
}

fn existing_score(name: &str, query: &str) -> Option<u8> {
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

fn is_subsequence(name: &str, query: &str) -> bool {
    let mut remaining = name.chars();
    query
        .chars()
        .all(|character| remaining.any(|candidate| candidate == character))
}

fn equal_length_edit(left: &[u8], right: &[u8]) -> Option<u8> {
    let mut differing = left
        .iter()
        .zip(right)
        .enumerate()
        .filter(|(_, (left, right))| left != right)
        .map(|(position, _)| position);
    let Some(first) = differing.next() else {
        return Some(0);
    };
    let Some(second) = differing.next() else {
        return Some(1);
    };
    if differing.next().is_some() || second != first + 1 {
        return None;
    }
    (left[first] == right[second] && left[second] == right[first]).then_some(1)
}

/// Bounded ASCII edit distance of at most one, including adjacent transposition.
fn single_edit(left: &str, right: &str) -> Option<u8> {
    if !left.is_ascii() || !right.is_ascii() || left.len().abs_diff(right.len()) > 1 {
        return None;
    }
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() == right.len() {
        return equal_length_edit(left, right);
    }
    let (shorter, longer) = if left.len() < right.len() {
        (left, right)
    } else {
        (right, left)
    };
    let first_difference = shorter
        .iter()
        .zip(longer)
        .position(|(left, right)| left != right)
        .unwrap_or(shorter.len());
    (shorter[first_difference..] == longer[first_difference + 1..]).then_some(1)
}

fn typo_score(application: &Application, query: &str) -> Option<u8> {
    if query.len() > 64 || query.split_whitespace().count() > 4 {
        return None;
    }
    let mut edits = 0;
    for token in query.split_whitespace() {
        if application.normalized.contains(token) {
            continue;
        }
        if token.len() < 3 {
            return None;
        }
        edits += application
            .words
            .iter()
            .filter_map(|word| single_edit(word, token))
            .min()?;
        if edits > 1 {
            return None;
        }
    }
    Some(45)
}

fn search(
    applications: &[Application],
    query: &str,
    mode: Mode,
    results: &mut Vec<(Reverse<u8>, usize)>,
) {
    results.clear();
    if query.is_empty() {
        return;
    }
    for (identifier, application) in applications.iter().enumerate() {
        let score = existing_score(&application.normalized, query).or_else(|| match mode {
            Mode::Existing => None,
            Mode::Subsequence => is_subsequence(&application.normalized, query).then_some(50),
            Mode::InitialsAndTypo => {
                (query.len() >= 2 && application.initials == query).then_some(55)
            }
        });
        if let Some(score) = score {
            results.push((Reverse(score), identifier));
        }
    }
    if results.is_empty() && matches!(mode, Mode::InitialsAndTypo) {
        for (identifier, application) in applications.iter().enumerate() {
            if let Some(score) = typo_score(application, query) {
                results.push((Reverse(score), identifier));
            }
        }
    }
    let compare = |left: &(Reverse<u8>, usize), right: &(Reverse<u8>, usize)| {
        left.0
            .cmp(&right.0)
            .then_with(|| {
                applications[left.1]
                    .normalized
                    .cmp(&applications[right.1].normalized)
            })
            .then(left.1.cmp(&right.1))
    };
    if results.len() > 8 {
        results.select_nth_unstable_by(7, compare);
        results.truncate(8);
    }
    results.sort_unstable_by(compare);
}

fn catalog(count: usize) -> Vec<Application> {
    (0..count)
        .map(|identifier| {
            let name = NAMES[identifier % NAMES.len()];
            let display = if identifier < NAMES.len() {
                name.to_owned()
            } else {
                format!("{name} Edition {identifier:04}")
            };
            Application::new(display)
        })
        .collect()
}

fn quality_report() {
    let applications = catalog(NAMES.len());
    println!("mode,query,expected,actual,correct");
    for mode in Mode::ALL {
        for (query, expected) in INTENTS {
            let mut results = Vec::new();
            search(&applications, &query.to_lowercase(), mode, &mut results);
            let actual = results
                .first()
                .map(|(_, identifier)| applications[*identifier].display.as_str());
            println!(
                "{},{query},{},{},{}",
                mode.label(),
                expected.unwrap_or("<none>"),
                actual.unwrap_or("<none>"),
                actual == *expected
            );
        }
    }
}

fn benchmark(run: usize) {
    let applications = catalog(2_000);
    let queries: Vec<_> = INTENTS
        .iter()
        .map(|(query, _)| query.to_lowercase())
        .collect();
    let mut modes = Mode::ALL;
    let mode_count = modes.len();
    modes.rotate_left(run % mode_count);
    println!("mode,query,samples,median_ns,p95_ns");
    for mode in modes {
        let mut results = Vec::with_capacity(applications.len());
        for query in &queries {
            for _ in 0..32 {
                search(&applications, query, mode, &mut results);
            }
            let mut timings = Vec::with_capacity(201);
            for _ in 0..201 {
                let started = Instant::now();
                search(
                    black_box(&applications),
                    black_box(query),
                    mode,
                    &mut results,
                );
                black_box(&results);
                timings.push(started.elapsed().as_nanos());
            }
            timings.sort_unstable();
            println!(
                "{},{query},201,{},{}",
                mode.label(),
                timings[100],
                timings[190]
            );
        }
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("quality") => quality_report(),
        argument => benchmark(
            argument
                .and_then(|argument| argument.parse().ok())
                .unwrap_or(0),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_edit_covers_edit_types_and_rejects_two_errors() {
        for (left, right, expected) in [
            ("chrome", "chorme", Some(1)),
            ("firefox", "firefx", Some(1)),
            ("terminal", "terrminal", Some(1)),
            ("excel", "excwl", Some(1)),
            ("code", "code", Some(0)),
            ("code", "cxdy", None),
            ("café", "cafe", None),
            ("", "a", Some(1)),
        ] {
            assert_eq!(single_edit(left, right), expected);
            assert_eq!(single_edit(right, left), expected);
        }
    }

    #[test]
    fn curated_intent_contract_is_satisfied_by_initials_and_bounded_typo() {
        let applications = catalog(NAMES.len());
        for (query, expected) in INTENTS {
            let mut results = Vec::new();
            search(
                &applications,
                &query.to_lowercase(),
                Mode::InitialsAndTypo,
                &mut results,
            );
            assert_eq!(
                results
                    .first()
                    .map(|(_, identifier)| applications[*identifier].display.as_str()),
                *expected,
                "{query}"
            );
        }
    }
}
