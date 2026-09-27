//! Isolated CLI experiment. No changes to Core or the supplied Search implementation.
#[path = "../line_search/corpus.rs"]
#[allow(dead_code)]
mod corpus;

use memchr::{memchr, memchr_iter, memmem::Finder, memrchr};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

fn files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn search_file(path: &Path, finder: &Finder<'_>, output: &mut impl Write) -> io::Result<()> {
    let bytes = std::fs::read(path)?;
    let mut cursor = 0;
    let mut line_number = 1;
    while let Some(relative) = finder.find(&bytes[cursor..]) {
        let hit = cursor + relative;
        line_number += memchr_iter(b'\n', &bytes[cursor..hit]).count();
        let start =
            memrchr(b'\n', &bytes[cursor..hit]).map_or(cursor, |position| cursor + position + 1);
        let end = memchr(b'\n', &bytes[hit..]).map_or(bytes.len(), |offset| hit + offset);
        write!(output, "{}:{}:", path.display(), line_number)?;
        output.write_all(&bytes[start..end])?;
        output.write_all(b"\n")?;
        if end == bytes.len() {
            break;
        }
        cursor = end + 1;
        line_number += 1;
    }
    Ok(())
}

fn scan(root: &Path, query: &str) -> io::Result<()> {
    if query.is_empty() || query.contains(['\r', '\n']) {
        return Err(io::Error::other("expected a nonempty single-line literal"));
    }
    let finder = Finder::new(query.as_bytes());
    let mut output = BufWriter::new(io::stdout().lock());
    for path in files(root)? {
        search_file(&path, &finder, &mut output)?;
    }
    output.flush()
}

fn prepare(root: &Path, manifest: &Path) -> io::Result<()> {
    let source_root = root.join("source/data");
    let many_root = root.join("many/data");
    let single_root = root.join("single/data");
    for directory in [&source_root, &many_root, &single_root] {
        std::fs::create_dir_all(directory)?;
    }
    let source_manifest = std::fs::read_to_string(manifest)?;
    for (identifier, filename) in source_manifest.lines().enumerate() {
        let text = std::fs::read_to_string(filename.trim_start_matches('\u{feff}'))?;
        let canonical = text
            .lines()
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        std::fs::write(
            source_root.join(format!("source_{identifier:04}.txt")),
            canonical,
        )?;
    }
    let generated = corpus::synthetic(1280);
    for (identifier, file) in generated.files.iter().enumerate() {
        let start = generated.lines[file.first].first;
        let end = generated.lines[file.end - 1].end + 1;
        std::fs::write(
            many_root.join(format!("generated_{identifier:04}.txt")),
            &generated.bytes[start..end],
        )?;
    }
    std::fs::write(single_root.join("combined.txt"), &generated.bytes)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().collect();
    match arguments.as_slice() {
        [_, mode, root, manifest] if mode == "prepare" => {
            prepare(Path::new(root), Path::new(manifest))?
        }
        [_, mode, root, query] if mode == "scan" => scan(Path::new(root), query)?,
        _ => {
            return Err("usage: tool-search prepare ROOT SOURCE_MANIFEST | scan ROOT QUERY".into())
        }
    }
    Ok(())
}
