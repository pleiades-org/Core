# Signed updates

Core 2.1.0 checks `https://github.com/pleiades-org/Core/releases/latest/download/core-update.txt`
in the background when shown. Successful checks are cached for 24 hours; failed checks
retry after one hour. The deadline survives restarting Core. There is no polling while hidden.

Settings > Behaviour > Updates offers:

- **Automatic** (default): check and download a newer signed release, then install on exit.
- **Notify**: check the signed manifest without downloading or installing an executable.
- **Off**: no update network requests or installation, including an already staged download.

`@update` shows status. Enter restarts a staged update, or opens the release page when
only a notification is available. `@info` shows the installed version and the latest release
the last check read (from GitHub, or from the cached signed manifest between daily checks),
with Enter on the latest row behaving like `@update`. A read-only installation folder uses notification only.
Checks send ordinary HTTPS requests to GitHub and its release-asset hosts, without cookies
or authentication. Local search and calculations still work offline. This setting controls
updates; the separate exchange-rate and website-icon features retain their own networking.

## Trust and manifest format

`crates/launcher/assets/release-key.bin` contains the 64-byte P-256 public key, X followed by Y.
The private PKCS#8 DER key must stay outside the repository and outside GitHub Actions secrets.
Possession of a GitHub account alone is insufficient to sign an update. Keep an offline backup
of the private key; losing it prevents existing installations from trusting future releases.
These signatures do not provide Authenticode identity or remove SmartScreen warnings.

The signed bytes are UTF-8 without a BOM, **LF only**, in this exact order, including the LF
after the hash. The `signature` line is excluded from the signed payload:

```ini
version=2.1.0
path=/pleiades-org/Core/releases/download/v2.1.0/core-v2.exe
sha256=<64 lowercase hexadecimal characters>
signature=<base64 of the 64-byte ECDSA P-256/SHA-256 P1363 signature>
```

Versions are exactly three unsigned decimal integers. Prerelease suffixes, extra fields,
duplicate fields, different repositories, query strings, and path traversal are rejected.
The manifest is capped at 4 KiB and executables at 8 MiB. Redirects are bounded, HTTPS-only,
and restricted to GitHub's release hosts. Hashes and signatures are checked again before
replacement and before a guarded restart.

## Installation and recovery

The download is staged beside the current executable as `core-v2.update.exe` with its
signed manifest in `core-v2.update.txt`. The executable is unchanged while Core runs.
Before marking the download ready, an isolated, hidden startup probe checks that Windows
can load it and its UI message loop starts. The probe disables networking and real actions.
Normal exit or sign-out renames the original to `core-v2.previous.exe` and moves the
verified stage into place. If the second rename fails, the original path is restored.
No network work is performed during exit or sign-out. The installed path and the user's
Start with Windows registration do not change.

Before swapping, Core saves a copy of the known-good executable as `core-v2.updater.exe`.
For `@update`, that helper waits on the old process handle before launching the replacement.
The replacement also honors `--after-update <pid>` before acquiring the singleton. A random
named event acknowledges a visible, fully opaque window from the UI message loop. If the
replacement exits or fails to acknowledge within five seconds, the helper stops that process,
restores the backup, and launches it. An update installed on ordinary exit arms the same
monitor on its next startup. Settings-file and motion arguments survive the restart.

Keep `core-v2.previous.exe` until the next successful update. If Windows cannot load a new
executable at all (before its startup monitor can run), exit Core and rename the previous
executable back to `core-v2.exe`. A failed version may remain as `core-v2.failed.exe` for diagnosis.
The updater does not request elevation or write to another installation directory.

## Packaging and publishing

Use PowerShell 7. Set `CORE_RELEASE_SIGNING_KEY` to the private PKCS#8 file's **path** or pass
`-SigningKeyPath`. Never put the key bytes in a command, log, repository, or issue.

```powershell
cargo build --release --locked -p core-launcher-v2 --bin core-v2
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
.\scripts\run-release-checks.ps1 -Executable .\target\release\core-v2.exe -SigningKeyPath $env:CORE_RELEASE_SIGNING_KEY
.\scripts\package-release.ps1 -Candidate .\target\release\core-v2.exe -SigningKeyPath $env:CORE_RELEASE_SIGNING_KEY -SkipInteractive
```

The handoff tests use isolated copies and background dry-run windows, including successful
startup, timeout rollback, ordinary next-start recovery, a hidden startup probe, and tampering
between staging and restart. They do not close an
installed Core. Omit `-SkipInteractive` only after running the interactive release checks.
Packaging checks the executable's reported version, validation-record hashes, and signing-key
match. It produces a versioned ZIP, `core-v2.exe`, and `core-update.txt` in
`dist/Core-2.1.0-windows-x64/`. For a release tagged `v2.1.0`, upload **both standalone assets**
alongside the ZIP. The updater does not read manifests inside ZIP files. Publish a full release
(not a draft or prerelease) to make it available through GitHub's `latest` URL.

Future releases must increase the Cargo workspace version and use the same signing key.
Replacing the compiled public key requires a planned transition signed by the existing key.
