//! Runs a command in a Windows pseudo console (ConPTY), so it sees a real terminal: prompts,
//! colours, line editing and full-screen programs work, and keys typed into Core reach it as
//! they are pressed. Output streams back to the UI thread as terminal bytes.
//! The process runs in a job object so Esc can stop it together with everything it started.
//! When it exits, its console closes, as a terminal window's would.
use super::{
    environment,
    shells::{reported_directory, Invocation},
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::*,
        Storage::FileSystem::{ReadFile, WriteFile},
        System::{
            Console::{ClosePseudoConsole, CreatePseudoConsole, COORD, HPCON},
            JobObjects::{AssignProcessToJobObject, CreateJobObjectW, TerminateJobObject},
            Pipes::CreatePipe,
            Threading::*,
        },
        UI::WindowsAndMessaging::{PostMessageW, WM_APP},
    },
};

pub const COMMAND_OUTPUT: u32 = WM_APP + 14;
const READ_CHUNK: usize = 16 * 1024;
/// Room for keys the console has not read yet, so typing never blocks Core.
const INPUT_BUFFER_BYTES: u32 = 64 * 1024;
/// How long to wait for the console's last output after the command exits.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Exited(u32),
    Stopped,
}

/// Output since the last update, and the outcome once the command has exited and its output
/// has all arrived.
pub struct Update {
    pub output: Vec<u8>,
    pub outcome: Option<Outcome>,
    /// Where the command finished, when its shell reported it.
    pub directory: Option<PathBuf>,
}

#[derive(Default)]
struct Shared {
    output: Vec<u8>,
    outcome: Option<Outcome>,
    directory: Option<PathBuf>,
    stopping: bool,
    /// The console's output has ended.
    drained: bool,
    /// A notification is queued and not yet taken by the UI.
    notified: bool,
    /// The UI no longer wants notifications (the run was replaced or Core is closing).
    detached: bool,
}

/// The run's state, and a signal for when its output ends.
type SharedState = Arc<(Mutex<Shared>, Condvar)>;

struct OwnedHandle(HANDLE);
// Kernel handles may be used from any thread.
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() && !self.0 .0.is_null() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

struct PseudoConsole(HPCON);
// Closed once, by the thread that waits for the command.
unsafe impl Send for PseudoConsole {}
impl Drop for PseudoConsole {
    fn drop(&mut self) {
        unsafe { ClosePseudoConsole(self.0) }
    }
}

struct AttributeList(LPPROC_THREAD_ATTRIBUTE_LIST);
impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.0) }
    }
}

/// Where keys typed into Core go: the console's input. The view that sends them holds a clone.
#[derive(Clone)]
pub struct TerminalInput(Arc<OwnedHandle>);

impl TerminalInput {
    pub fn write(&self, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() {
            return Ok(());
        }
        let mut written = 0_u32;
        unsafe { WriteFile(self.0 .0, Some(bytes), Some(&mut written), None) }
            .map_err(|failure| format!("The command is no longer reading input: {failure}"))?;
        if written as usize == bytes.len() {
            Ok(())
        } else {
            Err("The command did not accept all of the input".into())
        }
    }
}

pub struct CommandRun {
    job: OwnedHandle,
    shared: SharedState,
    input: TerminalInput,
}

impl CommandRun {
    /// Starts `invocation` in `directory` on a console of `columns` × `rows` characters.
    pub fn start(
        window: HWND,
        invocation: &Invocation,
        directory: &Path,
        columns: u16,
        rows: u16,
    ) -> Result<Self, String> {
        let error = |what: &str, error: windows::core::Error| format!("Could not {what}: {error}");
        let (output_read, output_write) =
            pipe(0).map_err(|failure| error("create the output pipe", failure))?;
        let (input_read, input_write) =
            pipe(INPUT_BUFFER_BYTES).map_err(|failure| error("create the input pipe", failure))?;
        let size = COORD {
            X: columns.clamp(1, i16::MAX as u16) as i16,
            Y: rows.clamp(1, i16::MAX as u16) as i16,
        };
        let console = PseudoConsole(
            unsafe { CreatePseudoConsole(size, input_read.0, output_write.0, 0) }
                .map_err(|failure| error("create a console for the command", failure))?,
        );
        // The console keeps its own copies. Closing ours lets the output end when it closes.
        drop((input_read, output_write));
        let job = OwnedHandle(
            unsafe { CreateJobObjectW(None, PCWSTR::null()) }
                .map_err(|failure| error("create a job object", failure))?,
        );
        // A stale report from an earlier Core must not be read as this command's.
        let _ = std::fs::remove_file(&invocation.directory_report);
        let (process, main_thread) = spawn_suspended(invocation, directory, &console)?;
        if let Err(failure) = unsafe { AssignProcessToJobObject(job.0, process.0) } {
            unsafe {
                let _ = TerminateProcess(process.0, 1);
            }
            return Err(error("track the command", failure));
        }
        resume_command(main_thread.0, job.0)?;
        drop(main_thread);
        let shared: SharedState = Arc::default();
        let address = window.0 as usize;
        {
            let shared = shared.clone();
            thread::Builder::new()
                .name("core-command-output".into())
                .spawn(move || read_output(output_read, shared, address))
                .map_err(|failure| {
                    failed_start(job.0, format!("Could not read command output: {failure}"))
                })?;
        }
        {
            let shared = shared.clone();
            let report = invocation.directory_report.clone();
            thread::Builder::new()
                .name("core-command-exit".into())
                .spawn(move || wait_for_exit(process, console, report, shared, address))
                .map_err(|failure| {
                    failed_start(job.0, format!("Could not watch the command: {failure}"))
                })?;
        }
        Ok(Self {
            job,
            shared,
            input: TerminalInput(Arc::new(input_write)),
        })
    }

    pub fn input(&self) -> TerminalInput {
        self.input.clone()
    }

    /// Stops a running command and everything it started. Returns false if it had finished.
    pub fn stop(&self) -> bool {
        let mut shared = self.shared.0.lock().expect("command lock");
        if shared.outcome.is_some() {
            return false;
        }
        shared.stopping = true;
        if let Err(error) = unsafe { TerminateJobObject(self.job.0, 1) } {
            eprintln!("Could not stop the command: {error}");
        }
        true
    }

    pub fn is_running(&self) -> bool {
        self.shared
            .0
            .lock()
            .expect("command lock")
            .outcome
            .is_none()
    }

    pub fn take_update(&self) -> Update {
        let mut shared = self.shared.0.lock().expect("command lock");
        shared.notified = false;
        Update {
            output: std::mem::take(&mut shared.output),
            outcome: shared.outcome.clone(),
            directory: shared.directory.take(),
        }
    }
}

impl Drop for CommandRun {
    fn drop(&mut self) {
        // Stopping lets the waiting thread close the console, which ends the output reader.
        self.stop();
        self.shared.0.lock().expect("command lock").detached = true;
    }
}

fn resume_command(thread: HANDLE, job: HANDLE) -> Result<(), String> {
    if unsafe { ResumeThread(thread) } == u32::MAX {
        let error = windows::core::Error::from_win32();
        return Err(failed_start(
            job,
            format!("Could not resume the command: {error}"),
        ));
    }
    Ok(())
}

fn failed_start(job: HANDLE, message: String) -> String {
    if let Err(error) = unsafe { TerminateJobObject(job, 1) } {
        return format!("{message}; could not terminate the command job: {error}");
    }
    message
}

fn pipe(buffer: u32) -> windows::core::Result<(OwnedHandle, OwnedHandle)> {
    let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
    unsafe { CreatePipe(&mut read, &mut write, None, buffer) }?;
    Ok((OwnedHandle(read), OwnedHandle(write)))
}

/// Creates the process attached to `console`, with its first thread suspended so it can join
/// the job before running. Returns the process and that thread.
fn spawn_suspended(
    invocation: &Invocation,
    directory: &Path,
    console: &PseudoConsole,
) -> Result<(OwnedHandle, OwnedHandle), String> {
    let error = |what: &str, error: windows::core::Error| format!("Could not {what}: {error}");
    let mut attribute_size = 0_usize;
    // The first call only reports the size needed, so its error is expected.
    let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut attribute_size) };
    let mut attribute_buffer = vec![0_u8; attribute_size];
    let attributes = LPPROC_THREAD_ATTRIBUTE_LIST(attribute_buffer.as_mut_ptr().cast());
    unsafe { InitializeProcThreadAttributeList(Some(attributes), 1, None, &mut attribute_size) }
        .map_err(|failure| error("prepare the command's console", failure))?;
    let _attribute_list = AttributeList(attributes);
    unsafe {
        UpdateProcThreadAttribute(
            attributes,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            Some(console.0 .0 as *const _),
            std::mem::size_of::<HPCON>(),
            None,
            None,
        )
    }
    .map_err(|failure| error("attach the command's console", failure))?;
    let startup = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOEXW>() as u32,
            // Without this the command could inherit Core's redirected output (its log file)
            // instead of using the console.
            dwFlags: STARTF_USESTDHANDLES,
            hStdInput: INVALID_HANDLE_VALUE,
            hStdOutput: INVALID_HANDLE_VALUE,
            hStdError: INVALID_HANDLE_VALUE,
            ..Default::default()
        },
        lpAttributeList: attributes,
    };
    let environment = environment::fresh_block(&invocation.environment);
    let program = wide_null(invocation.program.as_os_str());
    let mut command_line: Vec<u16> = invocation
        .command_line()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let directory = wide_null(directory.as_os_str());
    let mut flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED;
    if environment.is_some() {
        flags |= CREATE_UNICODE_ENVIRONMENT;
    }
    let mut information = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR(program.as_ptr()),
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            false,
            flags,
            environment.as_ref().map(|block| block.as_ptr().cast()),
            PCWSTR(directory.as_ptr()),
            &startup.StartupInfo,
            &mut information,
        )
    }
    .map_err(|failure| error(&format!("start {}", invocation.program.display()), failure))?;
    Ok((
        OwnedHandle(information.hProcess),
        OwnedHandle(information.hThread),
    ))
}

fn notify(shared: &mut Shared, address: usize) {
    if shared.notified || shared.detached {
        return;
    }
    shared.notified = true;
    if let Err(error) = unsafe {
        PostMessageW(
            Some(HWND(address as *mut _)),
            COMMAND_OUTPUT,
            WPARAM(0),
            LPARAM(0),
        )
    } {
        eprintln!("Could not deliver command output: {error}");
    }
}

fn read_output(read: OwnedHandle, shared: SharedState, address: usize) {
    let mut buffer = vec![0_u8; READ_CHUNK];
    loop {
        let mut count = 0_u32;
        let result = unsafe { ReadFile(read.0, Some(&mut buffer), Some(&mut count), None) };
        if result.is_err() || count == 0 {
            break;
        }
        let mut state = shared.0.lock().expect("command lock");
        state.output.extend_from_slice(&buffer[..count as usize]);
        notify(&mut state, address);
    }
    shared.0.lock().expect("command lock").drained = true;
    shared.1.notify_all();
}

/// Waits for the command, then closes its console so its last output arrives, and publishes
/// the outcome with the directory the shell reported.
fn wait_for_exit(
    process: OwnedHandle,
    console: PseudoConsole,
    report: PathBuf,
    shared: SharedState,
    address: usize,
) {
    unsafe {
        WaitForSingleObject(process.0, INFINITE);
    }
    let mut code = 0_u32;
    let exit_code = unsafe { GetExitCodeProcess(process.0, &mut code) }.map(|_| code);
    drop(console);
    let directory = read_report(&report);
    let (lock, drained) = &*shared;
    let state = lock.lock().expect("command lock");
    let (mut state, _) = drained
        .wait_timeout_while(state, DRAIN_TIMEOUT, |state| !state.drained)
        .expect("command lock");
    state.directory = directory;
    state.outcome = Some(if state.stopping {
        Outcome::Stopped
    } else {
        Outcome::Exited(exit_code.unwrap_or(1))
    });
    notify(&mut state, address);
}

/// The directory the shell wrote, removing its file.
fn read_report(report: &Path) -> Option<PathBuf> {
    let bytes = std::fs::read(report).ok()?;
    if let Err(error) = std::fs::remove_file(report) {
        eprintln!("Could not remove {}: {error}", report.display());
    }
    reported_directory(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}'))
}

fn wide_null(text: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    text.encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::super::{shells, terminal::Terminal};
    use super::*;
    use core_engine::search::ShellKind;
    use std::time::Instant;

    const COLUMNS: u16 = 100;
    const ROWS: u16 = 30;

    fn invocation(shell: ShellKind, command: &str) -> Invocation {
        shells::capture_invocation(&shells::resolve(shell).unwrap(), command)
    }

    /// A run and the screen its output draws, as Core shows it.
    struct Session {
        run: CommandRun,
        terminal: Terminal,
        directory: Option<PathBuf>,
    }

    impl Session {
        /// A null window makes notifications harmless thread messages.
        fn start(invocation: &Invocation, directory: &Path) -> Self {
            Self {
                run: CommandRun::start(HWND::default(), invocation, directory, COLUMNS, ROWS)
                    .unwrap(),
                terminal: Terminal::new(COLUMNS as usize, ROWS as usize),
                directory: None,
            }
        }

        fn poll(&mut self) -> Option<Outcome> {
            let update = self.run.take_update();
            self.terminal.feed(&update.output);
            self.run.input().write(&self.terminal.take_responses()).ok();
            self.directory = update.directory.or(self.directory.take());
            update.outcome
        }

        fn wait_for(&mut self, expected: &str) {
            let started = Instant::now();
            while !self.terminal.text().contains(expected) {
                self.poll();
                assert!(
                    started.elapsed() < Duration::from_secs(20),
                    "never saw {expected:?} in {:?}",
                    self.terminal.text()
                );
                thread::sleep(Duration::from_millis(20));
            }
        }

        fn finish(&mut self, stop_after: Option<Duration>) -> Outcome {
            let started = Instant::now();
            loop {
                if let Some(outcome) = self.poll() {
                    return outcome;
                }
                if stop_after.is_some_and(|limit| started.elapsed() > limit) {
                    assert!(self.run.stop());
                }
                assert!(
                    started.elapsed() < Duration::from_secs(20),
                    "command did not finish: {:?}",
                    self.terminal.text()
                );
                thread::sleep(Duration::from_millis(20));
            }
        }
    }

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        crate::windows::GUI_RESOURCE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn suspended_job() -> (OwnedHandle, OwnedHandle, OwnedHandle) {
        let program =
            PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/cmd.exe");
        let program = wide_null(program.as_os_str());
        let mut command: Vec<u16> = "cmd.exe /c exit 0".encode_utf16().chain(Some(0)).collect();
        let startup = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut process = PROCESS_INFORMATION::default();
        unsafe {
            CreateProcessW(
                PCWSTR(program.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_SUSPENDED | CREATE_NO_WINDOW,
                None,
                PCWSTR::null(),
                &startup,
                &mut process,
            )
        }
        .unwrap();
        let process_handle = OwnedHandle(process.hProcess);
        let thread_handle = OwnedHandle(process.hThread);
        let job = OwnedHandle(unsafe { CreateJobObjectW(None, PCWSTR::null()) }.unwrap());
        unsafe { AssignProcessToJobObject(job.0, process_handle.0) }.unwrap();
        (job, process_handle, thread_handle)
    }

    #[test]
    fn startup_errors_terminate_the_job_and_successful_resume_runs() {
        let _serial = lock();
        let (job, process, main_thread) = suspended_job();
        resume_command(main_thread.0, job.0).unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(process.0, 5_000) },
            WAIT_OBJECT_0
        );
        let (job, process, _main_thread) = suspended_job();
        assert!(resume_command(HANDLE::default(), job.0).is_err());
        assert_eq!(
            unsafe { WaitForSingleObject(process.0, 5_000) },
            WAIT_OBJECT_0
        );
        let (job, process, _main_thread) = suspended_job();
        assert_eq!(
            failed_start(job.0, "worker spawn failed".into()),
            "worker spawn failed"
        );
        assert_eq!(
            unsafe { WaitForSingleObject(process.0, 5_000) },
            WAIT_OBJECT_0
        );
    }

    #[test]
    fn output_exit_codes_and_unicode_are_shown() {
        let _serial = lock();
        let mut session = Session::start(
            &invocation(
                ShellKind::Cmd,
                "echo core-output & echo café 1>&2 & exit /b 3",
            ),
            &environment::home(),
        );
        assert_eq!(session.finish(None), Outcome::Exited(3));
        let text = session.terminal.text();
        assert!(text.contains("core-output"), "{text:?}");
        assert!(text.contains("café"), "{text:?}");
    }

    #[test]
    fn powershell_errors_are_plain_text_and_set_the_exit_code() {
        let _serial = lock();
        let mut session = Session::start(
            &invocation(
                ShellKind::WindowsPowerShell,
                "'café \"quoted\"'; Get-Item nope",
            ),
            &environment::home(),
        );
        assert_eq!(session.finish(None), Outcome::Exited(1));
        let text = session.terminal.text();
        assert!(text.contains("café \"quoted\""), "{text:?}");
        assert!(text.contains("Get-Item nope"), "{text:?}");
        assert!(
            !text.contains("CLIXML") && !text.contains("<Objs"),
            "{text:?}"
        );
        let mut handled = Session::start(
            &invocation(
                ShellKind::WindowsPowerShell,
                "try { Get-Item nope -ErrorAction Stop } catch { 'handled' }",
            ),
            &environment::home(),
        );
        assert_eq!(handled.finish(None), Outcome::Exited(0));
    }

    #[test]
    fn shells_report_the_directory_a_command_finished_in() {
        let _serial = lock();
        let windows = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
        // Windows paths ignore case, and shells report a folder's own casing.
        let same = |reported: &Option<PathBuf>, expected: &Path| {
            reported.as_ref().is_some_and(|path| {
                path.to_string_lossy()
                    .eq_ignore_ascii_case(&expected.to_string_lossy())
            })
        };
        // Git Bash is checked where it is installed.
        let shells = [
            ShellKind::Cmd,
            ShellKind::WindowsPowerShell,
            ShellKind::GitBash,
        ];
        for shell in shells
            .into_iter()
            .filter(|&shell| shells::resolve(shell).is_ok())
        {
            let command = invocation(shell, "cd System32");
            let mut session = Session::start(&command, &windows);
            assert_eq!(session.finish(None), Outcome::Exited(0), "{shell:?}");
            assert!(
                same(&session.directory, &windows.join("System32")),
                "{shell:?} {:?}",
                session.directory
            );
            assert!(
                !command.directory_report.exists(),
                "the report file is removed"
            );
            assert!(
                !session.terminal.text().contains("System32"),
                "{:?}",
                session.terminal.text()
            );
            // A failed command keeps its exit code alongside the report.
            let mut failed = Session::start(&invocation(shell, "cd nowhere-core"), &windows);
            assert_eq!(failed.finish(None), Outcome::Exited(1), "{shell:?}");
            assert!(
                same(&failed.directory, &windows),
                "{shell:?} {:?}",
                failed.directory
            );
        }
    }

    #[test]
    fn stopping_ends_the_command_and_its_children() {
        let _serial = lock();
        let started = Instant::now();
        let mut session = Session::start(
            &invocation(ShellKind::Cmd, "ping -n 30 127.0.0.1"),
            &environment::home(),
        );
        assert_eq!(
            session.finish(Some(Duration::from_millis(500))),
            Outcome::Stopped
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn prompts_wait_for_a_typed_answer() {
        let _serial = lock();
        let mut session = Session::start(
            &invocation(
                ShellKind::Cmd,
                "set /p answer=Name? & call echo got %^answer%",
            ),
            &environment::home(),
        );
        session.wait_for("Name?");
        session
            .run
            .input()
            .write("Robert café\r".as_bytes())
            .unwrap();
        session.wait_for("got Robert café");
        assert_eq!(session.finish(None), Outcome::Exited(0));
        // The console echoes typed text, as a terminal does. cmd keeps the space before `&`.
        assert!(
            session.terminal.text().contains("Name? Robert café"),
            "{:?}",
            session.terminal.text()
        );
    }

    #[test]
    fn single_keys_reach_the_program_without_enter() {
        let _serial = lock();
        let mut session = Session::start(
            &invocation(ShellKind::Cmd, "choice /c yn /m Continue"),
            &environment::home(),
        );
        session.wait_for("Continue");
        session.run.input().write(b"n").unwrap();
        assert_eq!(session.finish(None), Outcome::Exited(2));
        assert!(session.run.input().write(b"late").is_err());
    }
}
