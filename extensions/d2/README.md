# Destiny 2 extension assets

Destiny 2 search, manifest sync, and weapon logic live in the main crate at
[`src/destiny.rs`](../../src/destiny.rs). This folder only keeps static data used by that module.

## Contents

- `data/` — bundled icons, watermark-to-season mappings, and trait enhancement tables.

There is no separate Rust crate here; do not add another `destiny.rs` implementation under this tree.