using System;
using System.IO;

// `/command` runs in a --dry-run --test-commands instance: hidden runs only, so nothing opens a
// window or takes the foreground. Terminal and administrator runs are verified, not started.
public static class CommandsHarness
{
    private const int InputId = 100;
    private const int ResultsId = 101;
    private const int FooterId = 102;
    private const int OutputId = 103;
    private const int KeyUp = 0x26;
    private const int KeyDown = 0x28;
    private const int KeyBackspace = 0x08;

    private static string Footer(IntPtr window) { return WindowsHarness.Text(WindowsHarness.GetDlgItem(window, FooterId)); }
    private static string Output(IntPtr window) { return WindowsHarness.Text(WindowsHarness.GetDlgItem(window, OutputId)); }
    private static string Input(IntPtr window) { return WindowsHarness.Text(WindowsHarness.GetDlgItem(window, InputId)); }
    private static bool Visible(IntPtr window, int identifier) { return WindowsHarness.IsWindowVisible(WindowsHarness.GetDlgItem(window, identifier)); }

    private static void Key(IntPtr window, int key) {
        WindowsHarness.PostMessageW(WindowsHarness.GetDlgItem(window, InputId), 0x0100, new IntPtr(key), IntPtr.Zero);
    }

    // Typed characters, posted to Core's terminal as the keyboard delivers them.
    private static void Type(IntPtr window, string text) {
        foreach (char character in text)
            WindowsHarness.PostMessageW(WindowsHarness.GetDlgItem(window, OutputId), 0x0102, new IntPtr(character), IntPtr.Zero);
    }

    private static void Run(IntPtr window, string query) {
        WindowsHarness.Query(window, query);
        // The run modes still exist as results; terminal mode just does not show them as rows.
        WindowsHarness.WaitFor(() => WindowsHarness.Count(window) >= 3 && WindowsHarness.Row(window, 1) == "Open in terminal", "Command rows did not appear for " + query);
        WindowsHarness.WaitFor(() => !Visible(window, ResultsId), "Result rows are visible in / mode for " + query);
        WindowsHarness.Enter(window);
    }

    private static void Recall(IntPtr window, int key, string expected) {
        Key(window, key);
        WindowsHarness.WaitFor(() => Input(window) == expected, "History recall showed '" + Input(window) + "' instead of '" + expected + "'");
    }

    public static void Verify(IntPtr window, string settingsFolder) {
        WindowsHarness.Show(window);
        WindowsHarness.WaitFor(() => WindowsHarness.IsWindowVisible(window), "Core did not show");

        // Typing / enters command mode: the / is hidden and the box keeps only the command.
        Run(window, "/echo core-command-test");
        WindowsHarness.WaitFor(() => Footer(window).StartsWith("Done in"), "Command did not finish: " + Footer(window));
        if (!Output(window).Contains("core-command-test"))
            throw new InvalidOperationException("Command output missing: " + Output(window));
        if (Output(window).Contains("CLIXML") || Output(window).Contains("<Objs"))
            throw new InvalidOperationException("PowerShell serialization leaked into the output: " + Output(window));
        if (!Visible(window, OutputId))
            throw new InvalidOperationException("Output is hidden in command mode");
        WindowsHarness.WaitFor(() => Input(window) == "", "The command was not cleared after running: " + Input(window));

        // Still in command mode, so the next commands need no /.
        Run(window, "exit 3");
        WindowsHarness.WaitFor(() => Footer(window).StartsWith("Exit code 3"), "Exit code not reported: " + Footer(window));

        Run(window, "ping -n 30 127.0.0.1");
        WindowsHarness.WaitFor(() => Output(window).Contains("Reply from"), "Running output did not stream");
        WindowsHarness.Escape(window);
        WindowsHarness.WaitFor(() => Footer(window).StartsWith("Stopped"), "Esc did not stop the command: " + Footer(window));
        if (!WindowsHarness.IsWindowVisible(window)) throw new InvalidOperationException("Esc hid Core instead of stopping the command");

        string history = File.ReadAllText(Path.Combine(settingsFolder, "command-history.txt"));
        if (!history.Contains("echo core-command-test") || !history.StartsWith("ping -n 30 127.0.0.1"))
            throw new InvalidOperationException("Command history was not saved newest first: " + history);

        // Up and Down step through history like a shell prompt; typed text filters it.
        WindowsHarness.Query(window, "");
        Recall(window, KeyUp, "ping -n 30 127.0.0.1");
        Recall(window, KeyUp, "exit 3");
        Recall(window, KeyDown, "ping -n 30 127.0.0.1");
        Recall(window, KeyDown, "");
        WindowsHarness.Query(window, "ec");
        Recall(window, KeyUp, "echo core-command-test");

        // cd carries over to the next command, as in a terminal.
        string windows = Environment.GetFolderPath(Environment.SpecialFolder.Windows);
        Run(window, "cd '" + windows + "'");
        WindowsHarness.WaitFor(() => Output(window).Contains("> cd '" + windows + "'") && Footer(window).StartsWith("Done in"), "cd did not finish: " + Footer(window));
        Run(window, "cd System32");
        string system = (windows + "\\System32").ToLowerInvariant();
        // The working directory is shown in the prompt; the footer now contains only status.
        WindowsHarness.WaitFor(() => Output(window).ToLowerInvariant().StartsWith((windows + "> cd System32").ToLowerInvariant()), "A relative cd did not start from the previous directory: " + Output(window));
        WindowsHarness.WaitFor(() => Footer(window).StartsWith("Done in"), "Relative cd did not finish: " + Footer(window));
        Run(window, "echo here");
        WindowsHarness.WaitFor(() => Output(window).ToLowerInvariant().StartsWith(system + "> echo here"), "The prompt does not show the working directory: " + Output(window));
        WindowsHarness.WaitFor(() => Footer(window).StartsWith("Done"), "echo did not finish: " + Footer(window));

        // Backspace in the empty prompt removes the hidden / and returns to search.
        WindowsHarness.Query(window, "");
        Key(window, KeyBackspace);
        WindowsHarness.WaitFor(() => !Visible(window, OutputId), "Backspace in the empty prompt did not return to search");
        WindowsHarness.Query(window, "xbox");
        WindowsHarness.WaitFor(() => !Visible(window, OutputId) && WindowsHarness.First(window) != "xbox", "Backspace did not leave command mode");

        // A visible shell prefix stays after the run, ready for the next command.
        Run(window, "@cmd exit /b 3");
        WindowsHarness.WaitFor(() => Footer(window).StartsWith("Exit code 3"), "@cmd exit code not reported: " + Footer(window));
        WindowsHarness.WaitFor(() => Input(window) == "@cmd ", "@cmd prefix was not kept after running: " + Input(window));

        // Keys typed into the output reach the running command, as in a terminal: a line
        // answering a prompt...
        Run(window, "@cmd set /p answer=Name? & call echo got %^answer%");
        // The prompt line repeats the command, so wait for cmd's own prompt on the next line.
        WindowsHarness.WaitFor(() => Output(window).Contains("\nName?") && Footer(window) == "Esc to stop", "The command did not wait for input: " + Footer(window));
        Type(window, "Robert\r");
        WindowsHarness.WaitFor(() => Output(window).Contains("got Robert") && Footer(window).StartsWith("Done"), "Typed input did not reach the command: " + Output(window));
        // Typed input may be a password, so it is never saved as a command.
        if (File.ReadAllText(Path.Combine(settingsFolder, "command-history.txt")).Contains("Robert"))
            throw new InvalidOperationException("Typed input was saved to the command history");
        // ...and single keys, without Enter.
        Run(window, "@cmd choice /c yn /m Continue");
        WindowsHarness.WaitFor(() => Output(window).Contains("Continue [Y,N]?"), "choice did not ask: " + Output(window));
        Type(window, "n");
        WindowsHarness.WaitFor(() => Footer(window).StartsWith("Exit code 2"), "A single key did not answer choice: " + Footer(window));
        // Once the command has finished, typing goes to the search box for the next command.
        Type(window, "v");
        WindowsHarness.WaitFor(() => Input(window) == "@cmd v", "Typing after the command finished did not reach the search box: " + Input(window));

        // A full-screen program (Windows 11's edit) keeps Esc; Esc in the search box stops it.
        if (File.Exists(Path.Combine(Environment.SystemDirectory, "edit.exe"))) {
            Run(window, "@cmd edit");
            WindowsHarness.WaitFor(() => Footer(window) == "Esc in search to stop", "edit did not take the full screen: " + Footer(window));
            Type(window, "core-typed");
            WindowsHarness.WaitFor(() => Output(window).Contains("core-typed"), "Typing did not reach edit: " + Output(window));
            WindowsHarness.PostMessageW(WindowsHarness.GetDlgItem(window, OutputId), 0x0100, new IntPtr(27), IntPtr.Zero);
            System.Threading.Thread.Sleep(500);
            if (Footer(window) != "Esc in search to stop")
                throw new InvalidOperationException("Esc in the output stopped a full-screen program: " + Footer(window));
            WindowsHarness.Escape(window);
            WindowsHarness.WaitFor(() => Footer(window).StartsWith("Stopped") && !Output(window).Contains("core-typed"), "Esc in the search box did not stop edit and restore the output: " + Output(window));
        }

        WindowsHarness.Query(window, "@run shell:startup");
        WindowsHarness.WaitFor(() => WindowsHarness.First(window) == "Open shell:startup" && WindowsHarness.Row(window, 1) == "Run as administrator", "Run-dialog rows missing");
        WindowsHarness.Query(window, "ms-settings:display");
        WindowsHarness.WaitFor(() => WindowsHarness.First(window) == "Open ms-settings:display", "Bare Run-dialog URI was not recognised");
        WindowsHarness.Enter(window);
        WindowsHarness.WaitFor(() => Footer(window).Contains("no side effect"), "Dry run opened a Run-dialog target");
    }
}
