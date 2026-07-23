# Core

**Core** is a fast, keyboard-first Windows command launcher inspired by [Raycast](https://www.raycast.com/), built with Rust and [GPUI](https://www.gpui.rs/) from Zed Industries.

<img width="733" height="530" alt="image" src="https://github.com/user-attachments/assets/ddeb1860-08c7-4eaa-babc-098aaf943c59" />

---


## Features

### Universal Search & Actions
- **Apps** – Indexed Start Menu applications with icon display
- **Files** – Opt-in file indexing with content search, recent files, and file actions (copy path, copy name, show, delete)
- **Web** – Quick web searches via your default browser
- **Calculator** – Math, currency conversion, unit conversion, time zones, dates, percentages, tips, loans, and more
- **Clipboard History** – Text, links, colors, and image history with pin/delete/clear support
- **Emoji Picker** – Search and copy emojis (`:rocket` or `@emoji rocket`)

### Command Scopes (`@`)
| Scope | Description |
|---|---|
| `@app` | Search installed apps only |
| `@calc` | Calculator / time conversion |
| `@web` | Web search |
| `@files` / `@file` | Search indexed files |
| `@file:content` | Search inside file contents |
| `@video` / `@videos` | Search video files |
| `@images` / `@pictures` | Search image files |
| `@mp4`, `@pdf`, etc. | Filter by specific file type |
| `@note` | Create, export, delete, recover Markdown notes |
| `@focus` | Timed focus/blocking sessions |
| `@clip` / `@clipboard` | Clipboard history management |
| `@window` | Window management (left, right, thirds, center, etc.) |
| `@snippet` | Text expansion snippets (`;reply` to insert) |
| `@quicklink` | Saved quicklinks (`>docs` to open) |
| `@calendar` | Add events, join meetings |
| `@system` | System controls (lock, sleep, volume, brightness, etc.) |
| `@emoji` / `@e` | Emoji search |
| `@cmd` | In-launcher terminal with shell picker |
| `@custom` / `@alias` / `@hotkey` | Custom commands, aliases, and hotkeys |

### Built-in Terminal
Run commands in PowerShell, CMD, Git Bash, WSL, Zsh, or Fish directly inside the launcher with streaming output.

### Settings
Open the settings gear in the search bar (or run the built-in Open Settings action) to manage:

- General behavior (hotkey, startup, clipboard, web fallback)
- Search indexing (apps and files)
- Appearance (backdrop blur, display position)
- Shortcuts, expansions (aliases / snippets / quicklinks), and custom commands
- Advanced locale defaults (timezone, home currency)

You can also edit `%APPDATA%\Core Launcher\config.toml` directly:



```toml
hotkey = "Alt+Space"

[[aliases]]
keyword = "g"
expands_to = "@web"

[[custom_commands]]
name = "List repo"
description = "List files in the current repository"
command = "dir"
aliases = ["ls"]
hotkey = "Ctrl+Alt+L"
working_directory = "C:\\Users\\Robert\\coding\\Rust\\core"

[[hotkeys]]
hotkey = "Ctrl+Alt+N"
query = "@note Scratch | ## Notes"
description = "Create a scratch note"
```

### Smart Calculations
`2 + 2` · `15% of 89.99` · `10 USD to GBP` · `quote BTC` · `stock AAPL` · `tip 20% on $45` · `loan $10000 at 6% for 5 years` · `1h 20m + 45m` · `2 cups to tbsp` · `5 km to mi` · `32 f to c` · `time in london` · `2pm pt to uk` · `next monday 9am london to tokyo`

---

## Getting Started

### Prerequisites
- [Rust](https://rustup.rs/) (latest stable)

### Run from source
```bash
cargo run
```

### Build installer
```powershell
powershell -ExecutionPolicy Bypass -File scripts\build-setup.ps1
```
Then run `dist\CoreLauncherSetup.exe` to install for the current user with automatic startup.

---

## Usage

| Action | Shortcut |
|---|---|
| Toggle launcher | `Alt+Space` (configurable) |
| Select / copy result | `Enter` |
| Delete previous word | `Ctrl+Backspace` |
| Go back / hide | `Esc` |
| Dismiss launcher | Click away |

---

## Project Structure

```
src/              – Application source code
core-types/       – Shared type definitions
extensions/       – Extension modules
installer/        – Windows installer scripts
scripts/          – Build and utility scripts
assets/           – Assets (icons, etc.)
data/             – Runtime data
terminals/        – Terminal integration
```

---

## Data & Configuration

- **Config file**: `%APPDATA%\Core Launcher\config.toml`
- **Feature data** (notes, snippets, quicklinks, calendar, focus, clipboard): `%APPDATA%\Core Launcher\`

---

## Uninstall

```powershell
powershell -ExecutionPolicy Bypass -File installer\uninstall-core-launcher.ps1
```

---

## Documentation

- [Feature & technical requirements](docs/requirements.md)
