# Core 2.3.0

Core now includes a native Windows installer styled with the application's own fonts, colours, controls and rounded corners. Run **Core-Setup-2.3.0.exe** to install for your current Windows account without administrator access.

- Adds a Start Menu shortcut, an optional desktop shortcut and a Windows Installed apps entry.
- Keeps existing settings, quicklinks and command history, and migrates an existing Start with Windows registration to the installed executable.
- Supports repair by rerunning setup and removal through Windows Installed apps. Removal keeps preferences and user-added files.
- Keeps the existing signed automatic updater and manual `@update` checks. Portable downloads remain available.

See the [installation guide](https://github.com/pleiades-org/Core/blob/main/docs/INSTALLER.md). Setup is a native `.exe` installer; Authenticode signing remains unconfigured.

## Validation

Installer validation covers cancellation, installation, repair, rollback after a locked-file failure, Start Menu and desktop shortcuts, shortcut ownership, startup migration, Windows registration, relocated uninstall and temporary-file cleanup, preservation of user data, and rejection of redirected paths. Tests use isolated Windows Temp folders and a unique HKCU test key.

Release validation records identify the tested executable by SHA-256. Interactive pointer, physical-keyboard, screen-reader and mixed-DPI acceptance checks remain manual.

- 342 default unit tests passed, plus the opt-in installed-version registration test. Eleven other opt-in tests remain excluded.
- Formatting and Clippy with warnings denied passed.
- All ten background Windows release suites passed, including signed update handoff, recovery and rollback.
- The packaged setup passed installation and removal tests after renaming the download, without explicit installation arguments. Dark and light captures verify the native controls and text.
