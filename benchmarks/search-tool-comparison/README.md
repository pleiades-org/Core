Content-search comparisons for the Core plan. Production Core and upstream Search are unchanged.

Build from the Core planning directory:

```powershell
cargo build --release --locked --offline --no-default-features --bin codesearch --manifest-path benchmarks/reference/Search/Cargo.toml --target-dir benchmarks/search-tool-comparison/search-target
cargo build --release --locked --offline --bin tool-search --manifest-path benchmarks/search-alternatives/Cargo.toml
benchmarks/search-alternatives/target/release/tool-search.exe prepare "$PWD/benchmarks/search-tool-comparison/fixtures" benchmarks/search-alternatives/results/line-search/corpus-manifest.txt
benchmarks/search-tool-comparison/run.ps1
benchmarks/search-tool-comparison/run.ps1 -Tuning -Samples 11
benchmarks/search-tool-comparison/run.ps1 -ProfileMemory -Samples 5
benchmarks/search-tool-comparison/check-results.ps1
```

Cargo may need permission to unpack its locked dependencies into the user cache. The synthetic fixture contains 1,280 files and 55,707,698 bytes; `single` is exactly those same bytes concatenated. `source` is the earlier experiment's 73-file source snapshot canonicalized to LF. Each tool receives the `data` directory from the same working directory, producing similarly sized relative path output. Search stores its filename database outside `data`. The benchmark requests all matching lines, not an early result limit.

`tools.ps1` records exact executable paths and arguments. All tests use case-sensitive single-line literals on valid UTF-8 text, no binary files, ignores, symlinks or mutation. Uppercase-query correctness is separately probed because the original Search lowercases only its query. `ResultSignature.cs` verifies full filename/line-number/content tuples after sorting; timing excludes sorting and hashing. An independent `ReadLines`/ordinal `Contains` oracle verifies results. Output is fully drained and UTF-8 decoded inside timing, with no terminal rendering. Exit codes are checked; no-match exit 1 is accepted only for the standard tools.

The main run rotates tool order and records 21 fresh-process samples per tool/query/corpus after an untimed correctness run. OS caches and Search's filename database are warm. These are not cold-storage or resident-engine timings. Search uses its supplied default release profile with the optional Windows MFT feature disabled so it searches only the test directory. The prototype uses the existing experimental crate's thin-LTO release profile; binaries and versions are recorded. It scans one file at a time with a reused `memmem` finder, skips directly past matched lines, and buffers stdout. It is single-threaded and reads a complete file into memory; it has no content index and is deliberately limited to these flat fixtures.

The tuning run interleaves unchanged Search and `rg-auto` controls with `rg --mmap`, `rg` supplied an already prepared explicit file list, and native Git without its Windows command launcher. Preparing the explicit list is untimed; OS process creation still serializes its arguments. Its approximately 31 KB command line fits this Windows fixture, but is not a scalable manifest interface for millions of paths. The tuning run has 11 samples per case. Do not treat small differences across separate runs as decisive.

Memory profiling is a separate five-sample pass. It polls the OS process working-set high-water mark approximately every 1 ms while the process is alive. These observed peaks may miss a brief final allocation and are lower bounds, not guaranteed whole-lifetime peaks or private heap bytes. They exclude the parent harness and filesystem cache. Main-run CPU time is the child process's total user+kernel time, with observed 15.625 ms quantization; the Windows Git launcher does not include its child's CPU, so use native Git resource results instead. Profiling wall times include sampling overhead and are not used for latency rankings.

`results/samples.csv` and `summary.csv` contain raw wall/CPU measurements and medians/p95. `tuning-*` and `memory-*` files keep other passes separate. `preparation.csv` is the untimed validation pass, not a guaranteed cold-cache measurement. `edge-validation.csv` records correctness probes, including expected failure of the original uppercase query. Zero memory in a non-profile run means unmeasured. The machine is not isolated or affinity-pinned. GNU grep is the installed Git-for-Windows/MSYS build, not a current Linux installation.
