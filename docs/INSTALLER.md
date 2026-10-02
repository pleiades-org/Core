# Installing Core

Run **Core-Setup-2.3.0.exe** and choose **Install**. Setup installs for the current Windows account without administrator access, then offers **Open Core**. It uses Core's own fonts, palette, buttons, switches and rounded corners. Existing appearance settings also apply to setup.

Core is installed in `%LOCALAPPDATA%\Programs\Pleiades\Core`. Setup adds **Core** to the Start Menu and registers **Core** under Windows **Settings → Apps → Installed apps**. A desktop shortcut is optional and initially off. Reinstalling repairs the installed files and remembers that choice.

If Core is running, exit it from its tray menu first. Setup reports the running app and offers **Retry**. Existing Start with Windows registration is moved to the installed executable; setup does not enable startup if it was off. You can change startup in Core's Behaviour settings.

Settings, quicklinks, history and logs remain in `%APPDATA%\Pleiades\Core\v2`. Moving from a portable copy keeps these preferences. After installation, use the Start Menu shortcut and remove the old portable executable when you no longer need it.

Uninstall from Windows **Installed apps**, or run the installed `uninstall.exe --uninstall`. Removal keeps preferences and user-added files. Setup never overwrites an unrelated shortcut: rename it if it conflicts with the selected installation options. Redirected installation folders and links are rejected rather than followed during file changes.

The current-user folder stays writable by Core's existing [signed automatic updater](UPDATES.md). Updates keep the executable path and shortcuts stable, and refresh the version shown by Windows on the next normal launch. A portable ZIP and standalone `core-v2.exe` remain available for existing updater clients and portable use.

The setup program is a native `.exe` installer. It requires 64-bit Windows and the Microsoft Visual C++ v14 x64 runtime, like the application. No additional installer framework or browser runtime is used. Releases have signed update manifests; Authenticode signing is not configured, so Windows may still show an unknown-publisher warning.

## Development and validation

Build the normal release executable, then run `core-v2.exe --installer-preview --test-background` to inspect setup without changing files. `--install` and `--uninstall` explicitly select setup modes. Packaging creates `Core-Setup-<version>.exe`, which enters installation mode automatically.

`scripts/test-installer.ps1 -Executable target/release/core-v2.exe` exercises cancellation, installation, repair, rollback after a locked-file failure, shortcut ownership, redirected-path rejection and both installed uninstall entry points. Every mutation is restricted to a uniquely named test folder under Windows Temp and an isolated HKCU test key. Tests do not install over an existing Core or change its real startup registration.

After packaging, also test the setup artifact with `-PayloadExecutable target/release/core-v2.exe -AutomaticSetup`. The setup contains an identifying PE overlay, which is removed from the installed application payload; it opens setup even when a browser renames the download. The standalone application's signed updater hash remains unchanged.
