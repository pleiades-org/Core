# Core 2.2.2

Pressing Enter on `@update` now checks GitHub immediately instead of waiting for the next daily check or the one-hour retry after an error. Manual checks are limited to once a minute, including repeated presses after a failed request.

- Automatic checks when Core is shown keep their existing daily schedule and one-hour failure backoff.
- A ready update still restarts and installs when accepted; a notification still opens the release page. Updates set to **Off** stay off.
- Earlier installed versions keep their old behavior until updated. Exit Core, replace `core-v2.exe` with the 2.2.2 release executable, and reopen it to get immediate manual checks on each PC.

See [the update guide](https://github.com/pleiades-org/Core/blob/main/docs/UPDATES.md).

## Validation

- 331 unit tests passed (130 engine, 201 launcher); eleven opt-in tests remain excluded from the default run. New scheduling tests cover manual checks during daily caching and failure backoff, the 59/60-second cooldown boundary, an expired automatic deadline, and blocked checks.
- Formatting and Clippy with warnings denied passed.
- All nine background Windows release suites passed on the packaged executable, including signed update handoff, verification, startup recovery, and rollback.
- Interactive click-away, pointer hover, and physical-keyboard checks were not run.
