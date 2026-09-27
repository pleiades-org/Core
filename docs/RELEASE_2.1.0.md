# Core 2.1.0

This release brings the native v2 launcher into the main Core repository and adds signed updates.

- Settings > Behaviour now offers Updates: Automatic, Notify, or Off.
- Background GitHub checks run when shown, once per day, with one-hour error backoff.
- Automatic updates verify ECDSA P-256 signatures and SHA-256 before staging and replacement.
- `@update` restarts a ready update or opens its release page. Normal exit installs a staged update.
- A known-good helper monitors updated startup and restores the backup after a five-second timeout.
- Read-only installations receive notifications instead of requesting administrator access.
- Search indexing retains long-name catalogs and indexes two-byte queries within the memory budget.
- Known-folder relocation refreshes recent-app resolutions without restarting Core.

See [UPDATES.md](UPDATES.md) for network behavior, signing, release assets, and recovery.
Executables remain unsigned by Authenticode. Background release checks do not replace manual
foreground, mixed-DPI, accessibility, or physical-keyboard validation.

## Validation

- 276 unit tests passed; eight opt-in tests remain excluded from the default run.
- `cargo fmt --all --check` and Clippy with warnings denied passed.
- All nine background release suites passed on the packaged executable. The settings suite
  needed one unchanged rerun after a Tab-focus mismatch; interactive focus checks remain manual.
- Updater checks cover an isolated hidden startup probe, running-image rename, visible-window
  acknowledgment, backup retention, and timeout/tampering rollback for explicit restart and
  the next ordinary startup. Unit tests cover signature rejection, version parsing, bounded
  reads, read-only detection, failed swaps, and leaving stages untouched under Notify/Off.
- The installed Core process was left running. Sign-out and interactive pointer/keyboard
  acceptance were not exercised on the user's desktop.
