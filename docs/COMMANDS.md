# Running commands from Core

Type `/` to switch the search box to a command prompt. The `/` is hidden, and everything typed after it goes to the shell unchanged, including a further `/`. The result rows disappear, and Core's terminal appears in their place. After a command starts, the box is cleared for the next one and stays a command prompt. Backspace in the empty prompt returns to search.

| Input | What happens |
| --- | --- |
| `/ipconfig /all`, then Enter | Runs in your shell, in Core's terminal |
| ↑ and ↓ in the prompt | Steps through recent commands, newest first; ↓ past the newest returns to what you typed |
| `git`, then ↑ | Steps through recent commands that start with `git` |
| `@cmd dir`, `@ps Get-Process`, `@powershell …`, `@pwsh …`, `@wsl ls`, `@bash ls` | The same, in a specific shell. The prefix stays visible, and after a run the box keeps it (`@cmd `) for the next command |
| `@run notepad`, `@run %appdata%` | Opens it like Windows Run (Win+R), with a **Run as administrator** row |
| `shell:startup`, `ms-settings:display`, `%temp%`, `C:\Users`, `\\nas\share` | Recognised without a prefix, because none can be an app name |

A command can run three ways:

| Keys | What happens |
| --- | --- |
| Enter | Runs in Core's terminal. Keys typed into it reach the command |
| Ctrl+Enter | Opens it in a terminal: Windows Terminal if installed, otherwise a console window. The shell stays open afterwards and loads your profile. |
| Ctrl+Shift+Enter | Opens it in a terminal as administrator, after the Windows consent prompt |

## Core's terminal

Commands run in a Windows pseudo console (ConPTY), the same kind of console Windows Terminal uses, so programs behave as they do in a terminal. Core draws the console's screen below the prompt line: colours, bold and underlined text, tables and progress that redraws in place.

- The output grows with its text; long output scrolls. The mouse wheel, the scroll bar and Shift+Page Up/Page Down scroll back through up to 5,000 lines. The footer shows the shell, then the exit code and duration.
- **Keys go straight to the running command.** When a command starts, the keyboard moves to the terminal, and every key reaches the program as you press it, as in a terminal:
  - answers to `set /p`, `Read-Host` and y/n questions;
  - single keys, for `choice` and "press any key";
  - line editing and history in REPLs such as `python`, `node` and `wsl`;
  - arrows, Home/End, Page Up/Page Down, Insert/Delete, F1–F12, Tab completion and Alt combinations.
  - Ctrl+C interrupts the program. With text selected, Ctrl+C copies it instead.
  Typed keys are never saved to the history, since they may be passwords.
- **Full-screen programs** (Windows 11's `edit`, `vim`, `less`, `htop` in WSL, `ssh`) take the whole terminal. Esc then belongs to the program; the footer says to use Esc in the search box to stop it.
- **Esc stops a running command** and everything it started; the next Esc hides Core as usual. In a full-screen program, Esc typed in the terminal goes to the program, and Esc in the search box stops it. Hiding Core does not stop a command. Its output is still there when you reopen Core with a `/` query, and the keyboard returns to the terminal while the command runs.
- **Selecting and copying:** drag to select text, then Ctrl+C, Ctrl+Insert or right-click to copy. Ctrl+Shift+C always copies. Ctrl+A selects everything once the command has finished.
- **Pasting:** Ctrl+V, Shift+Insert, or right-click with nothing selected pastes into the running command. Programs that ask for it are told the text was pasted, so a pasted command line is not run line by line.
- When a command finishes, the keyboard returns to the search box, and typing in the terminal moves there too. Enter in finished output does nothing, so a command is never rerun by accident. Enter in the search box starts the next command; if one is still running, it is stopped first.
- PowerShell errors appear in red, as they do in a terminal. The exit code is the last program's, or 1 when the last PowerShell statement failed.
- A window a command opens (`start notepad`) stays open. Console programs still attached when the command ends close with the terminal, as they do when a terminal window closes.
- Runs skip PowerShell profiles for speed and predictable output; terminal runs (Ctrl+Enter) load them.

## Which shell

**Settings → Behaviour → Commands run in** chooses the shell for `/`. **Default shell** follows Windows Terminal's default profile, which is what you get when you open a terminal: Command Prompt, Windows PowerShell, PowerShell 7, WSL (including a specific distribution) or Git Bash. If Windows Terminal is not installed, or its default profile is something Core cannot drive (such as Azure Cloud Shell or a custom shell), Windows PowerShell is used and the footer says so.

Every command gets a freshly built environment, the same way the Windows Run dialog does, so programs installed after Core started are found on PATH.

## Working directory

Commands start in your user profile folder. Each command runs in a new shell, but `cd` still carries over as it does in a terminal: the shell writes the folder it finished in to a temporary file, which Core reads and deletes, and the next command starts there. That also works for `cd src && ls`, `pushd` and `Set-Location`. The prompt line above each output shows the folder the command ran in, with `~` for your profile folder, and the footer shows the current folder. Ctrl+Enter and Ctrl+Shift+Enter open their terminal in the current folder too.

The folder is shared by every shell and resets to your profile folder when Core restarts, or when the folder no longer exists. Shell variables, aliases and functions do not carry over, because each command is a new shell. A command stopped with Esc keeps the previous folder.

## History

Every command you run is saved, newest first, in `%APPDATA%\Pleiades\Core\v2\command-history.txt`. At most 100 are kept, and duplicates move to the top. It is plain text, so avoid typing secrets into commands. **Settings → Behaviour → Clear command history** deletes it.

## Testing

`--dry-run` never runs commands or opens Run-dialog targets. `--dry-run --test-commands` allows runs in Core's terminal only; terminal-window and administrator runs stay verification-only. `scripts\test-commands.ps1` checks output, exit codes, keys typed into running commands, full-screen Esc handling, history recall and Run-dialog rows in background mode.
