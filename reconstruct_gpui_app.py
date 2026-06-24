#!/usr/bin/env python3
"""Reconstruct gpui_app.rs from launcher submodule pieces (undo bad split)."""
from pathlib import Path

ROOT = Path(__file__).resolve().parent
LAUNCHER = ROOT / "src" / "ui" / "launcher"
OUT = ROOT / "src" / "ui" / "gpui_app.rs"

mod_lines = (LAUNCHER / "mod.rs").read_text(encoding="utf-8").splitlines(keepends=True)
settings_lines = (LAUNCHER / "settings_panel.rs").read_text(encoding="utf-8").splitlines(keepends=True)[2:]  # skip use
result_lines = (LAUNCHER / "result_list.rs").read_text(encoding="utf-8").splitlines(keepends=True)[2:]
destiny_lines = (LAUNCHER / "destiny_detail.rs").read_text(encoding="utf-8").splitlines(keepends=True)

# Remove module declarations from mod.rs
mod_body = []
skip = {"mod destiny_detail;", "mod result_list;", "mod settings_panel;"}
for line in mod_lines:
    if line.strip() in skip:
        continue
    mod_body.append(line)

# mod.rs was built from these chunks in order - find split points by unique markers
mod_text = "".join(mod_body)

# Markers to reinsert extracted impl sections
pieces = []

# Split mod at impl LauncherView boundaries using known anchors
anchors = [
    "impl LauncherView {",
    "impl Focusable for LauncherView",
    "impl Render for LauncherView",
    "struct FileAssets",
]

# Simpler: use stored range reconstruction from split script
# Read original ranges and reassemble from mod + extracted files

def read_body(path, skip_header=2):
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
    return lines[skip_header:]

settings = read_body(LAUNCHER / "settings_panel.rs")
result = read_body(LAUNCHER / "result_list.rs")

# mod.rs content without mod declarations
mod_clean = []
for line in mod_lines:
    if line.strip() in skip:
        continue
    mod_clean.append(line)

# The mod.rs was assembled from ranges - we need original source.
# Concatenate: mod part1 + result impl1 + mod part2 + ... in script order

RANGES_ORIGINAL = {
    "mod": [
        (1, 512),
        (605, 635),
        (645, 655),
        (841, 947),
        (948, 1050),
        (1410, 1573),
        (1771, 1907),
        (1937, 2060),
        (2199, 2213),
        (2442, 2462),
        (2524, 2938),
    ],
    "settings": [(656, 840), (1574, 1770)],
    "result_impl": [(513, 604), (636, 644), (1051, 1409)],
    "result_free": [(1908, 1936), (2061, 2198), (2382, 2395), (2463, 2523)],
}

# mod.rs is concatenation of mod ranges - split mod_clean back using line counts
mod_range_lengths = [end - start + 1 for start, end in RANGES_ORIGINAL["mod"]]
mod_chunks = []
idx = 0
for length in mod_range_lengths:
    mod_chunks.append(mod_clean[idx : idx + length])
    idx += length

settings_chunks = []
sidx = 0
for start, end in RANGES_ORIGINAL["settings"]:
    length = end - start + 1
    settings_chunks.append(settings[sidx : sidx + length])
    sidx += length

result_impl_chunks = []
ridx = 0
for start, end in RANGES_ORIGINAL["result_impl"]:
    length = end - start + 1
    result_impl_chunks.append(result[ridx : ridx + length])
    ridx += length

result_free_chunks = []
for start, end in RANGES_ORIGINAL["result_free"]:
    length = end - start + 1
    result_free_chunks.append(result[ridx : ridx + length])
    ridx += length

# Reassemble in original file order
ordered = []
ordered.extend(mod_chunks[0])  # 1-512
ordered.extend(result_impl_chunks[0])  # 513-604
ordered.extend(mod_chunks[1])  # 605-635
ordered.extend(result_impl_chunks[1])  # 636-644
ordered.extend(mod_chunks[2])  # 645-655
ordered.extend(settings_chunks[0])  # 656-840
ordered.extend(mod_chunks[3])  # 841-947
ordered.extend(mod_chunks[4])  # 948-1050
ordered.extend(result_impl_chunks[2])  # 1051-1409
ordered.extend(mod_chunks[5])  # 1410-1573
ordered.extend(settings_chunks[1])  # 1574-1770
ordered.extend(mod_chunks[6])  # 1771-1907
ordered.extend(result_free_chunks[0])  # 1908-1936
ordered.extend(mod_chunks[7])  # 1937-2060
ordered.extend(result_free_chunks[1])  # 2061-2198
ordered.extend(mod_chunks[8])  # 2199-2213
ordered.extend(settings[sum(end - start + 1 for start, end in RANGES_ORIGINAL["settings"]) :])  # settings free funcs - wrong

print("Reconstruction via ordered chunks failed safely - use git restore instead")