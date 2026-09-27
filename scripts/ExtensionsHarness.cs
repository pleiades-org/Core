using System;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;

public static class ExtensionsHarness
{
    [StructLayout(LayoutKind.Sequential)] private struct Rectangle { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr window, out Rectangle bounds);
    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")] private static extern IntPtr TextMessage(IntPtr window, uint message, IntPtr word, string text);
    [DllImport("user32.dll")] private static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr word, IntPtr data);

    private static void Query(IntPtr window, string query, string expected) {
        WindowsHarness.Query(window, query);
        WindowsHarness.WaitFor(() => WindowsHarness.First(window) == expected, query + " did not return " + expected + "; got " + WindowsHarness.First(window));
        SettingsHarness.Flush(window);
    }
    public static void SetShortcut(IntPtr window, string text) {
        if (TextMessage(WindowsHarness.GetDlgItem(window, 240), 0x000C, IntPtr.Zero, text) == IntPtr.Zero) throw new InvalidOperationException("Shortcut edit failed");
        SettingsHarness.Flush(window);
        if (WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 240)) != text) throw new InvalidOperationException("Shortcut edit did not reach the native edit control");
    }
    private static int Selection(IntPtr window) { return SendMessageW(WindowsHarness.GetDlgItem(window, 101), 0x0188, IntPtr.Zero, IntPtr.Zero).ToInt32(); }

    public static void Verify(IntPtr window, string directory) {
        Query(window, "2 days from now", DateTime.Now.Date.AddDays(2).ToString("d MMMM yyyy", CultureInfo.InvariantCulture));
        WindowsHarness.Show(window);
        WindowsHarness.WaitFor(() => WindowsHarness.IsWindowVisible(window), "Core did not show");
        SettingsHarness.Flush(window);
        CaptureWindow.Save(window, Path.Combine(directory, "calculator-date.bmp"));
        Query(window, "2028-01-31 + 1 month", "29 February 2028");
        Query(window, "2026-12-25 - 2026-09-21", "95 days");
        Query(window, "2 days in hours", "48 h");
        Query(window, "100c to f", "212 °F");
        Query(window, "110 usd to eur", "€100.00");
        Query(window, "10 GB at 100 Mbps", "13 min 20 s");
        Query(window, "255 to hex", "0xFF");
        Query(window, "20% off 80", "64");
        Query(window, "#ff8800", "rgb(255, 136, 0)");
        Query(window, "9 pt to cup", "18 US cup");
        Query(window, "10 pounds to kg", "4.5359237 kg");
        Query(window, "@calc sqrt(81) + round(2.6)", "12");
        GetWindowRect(window, out Rectangle rich);
        int expectedHeight = (int)((258 * GetDpiForWindow(window) + 48) / 96);
        if (rich.Bottom - rich.Top != expectedHeight) throw new InvalidOperationException("Expanded answer card has the wrong height");
        CaptureWindow.Save(window, Path.Combine(directory, "calculator-answer.bmp"));
        WindowsHarness.Query(window, "sqrt(-1)");
        WindowsHarness.Enter(window);
        WindowsHarness.WaitFor(() => WindowsHarness.Count(window) == 0 && WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 102)).Contains("undefined"), "Invalid function retained an action");
        Query(window, "2+2", "4");
        GetWindowRect(WindowsHarness.GetDlgItem(window, 104), out Rectangle settings);
        GetWindowRect(WindowsHarness.GetDlgItem(window, 105), out Rectangle power);
        GetWindowRect(window, out Rectangle bounds);
        if (settings.Top >= bounds.Top + 80 || power.Top <= settings.Bottom) throw new InvalidOperationException("Settings and power are not in their requested corners");
        SettingsHarness.Click(window, 105);
        if (!WindowsHarness.IsWindowVisible(WindowsHarness.GetDlgItem(window, 107))) throw new InvalidOperationException("Inline power menu did not open");
        CaptureWindow.Save(window, Path.Combine(directory, "power-menu.bmp"));
        WindowsHarness.PostMessageW(WindowsHarness.GetDlgItem(window, 107), 0x0100, new IntPtr(40), IntPtr.Zero);
        SettingsHarness.Flush(window);
        if (WindowsHarness.FocusedControl(window) != WindowsHarness.GetDlgItem(window, 108)) throw new InvalidOperationException("Power menu arrow navigation failed");
        WindowsHarness.PostMessageW(WindowsHarness.GetDlgItem(window, 108), 0x0100, new IntPtr(27), IntPtr.Zero);
        SettingsHarness.Flush(window);
        if (!WindowsHarness.IsWindowVisible(window) || WindowsHarness.IsWindowVisible(WindowsHarness.GetDlgItem(window, 107))) throw new InvalidOperationException("Escape should close the menu first");
        foreach (int identifier in new[] {107, 108, 109}) {
            SettingsHarness.Click(window, 105);
            SettingsHarness.Click(window, identifier);
            WindowsHarness.WaitFor(() => WindowsHarness.First(window) == "Cancel" && WindowsHarness.Count(window) == 2, "Power confirmation was not displayed");
            SettingsHarness.Flush(window);
            if (Selection(window) != 0) throw new InvalidOperationException("Power confirmation must default to Cancel");
            WindowsHarness.Enter(window);
            WindowsHarness.WaitFor(() => WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 100)) == "", "Default Enter did not cancel power action");
        }
        Query(window, "@power", "Power off");
        WindowsHarness.Enter(window);
        WindowsHarness.WaitFor(() => WindowsHarness.First(window) == "Cancel", "Command did not ask for power confirmation");
        WindowsHarness.PostMessageW(WindowsHarness.GetDlgItem(window, 100), 0x0100, new IntPtr(40), IntPtr.Zero);
        SettingsHarness.Flush(window);
        WindowsHarness.Enter(window);
        WindowsHarness.WaitFor(() => WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 102)).Contains("no side effect"), "Confirmed power action did not reach dry-run dispatch");
        WindowsHarness.Query(window, "@app xbox");
        WindowsHarness.WaitFor(() => WindowsHarness.Count(window) > 0 && WindowsHarness.First(window).IndexOf("Xbox", StringComparison.OrdinalIgnoreCase) >= 0, "Installed Xbox app was not discovered");
        SettingsHarness.Flush(window);
        CaptureWindow.Save(window, Path.Combine(directory, "xbox-discovery.bmp"));
        SettingsHarness.Open(window);
        SettingsHarness.Click(window, 231);
        SettingsHarness.CheckSidebar(window, true);
        SetShortcut(window, "Ctrl");
        if (!SettingsHarness.IsOpen(window) || !WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 222)).Contains("Add a key")) throw new InvalidOperationException("Incomplete shortcut status: open=" + SettingsHarness.IsOpen(window) + ", text=" + WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 240)) + ", status=" + WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 222)));
        SetShortcut(window, "Ctrl+Shift+F11");
        SettingsHarness.Choose(window, 242, 1);
        if (WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 242)).StartsWith("Active")) throw new InvalidOperationException("Specific display option missing");
        SettingsHarness.Click(window, 243);
        CaptureWindow.Save(window, Path.Combine(directory, "settings-behaviour.bmp"));
        SettingsHarness.WaitSaved(window);
        SettingsHarness.Done(window);
        SettingsHarness.Open(window);
        SettingsHarness.Click(window, 231);
        if (WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 240)) != "Ctrl+Shift+F11" || !WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 243)).EndsWith("On")) throw new InvalidOperationException("Behaviour settings were not saved");
        SettingsHarness.Click(window, 241);
        if (WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 240)) != "Win" || !WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 241)).EndsWith("On")) throw new InvalidOperationException("Windows-key switch did not turn on");
        SettingsHarness.Click(window, 241);
        if (WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 240)) != "Ctrl+Alt+Space" || !WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 241)).EndsWith("Off")) throw new InvalidOperationException("Windows-key switch did not turn off");
        SettingsHarness.Choose(window, 247, 1);
        if (WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 247)) != "Command Prompt") throw new InvalidOperationException("Shell dropdown did not choose Command Prompt");
        SettingsHarness.WaitSaved(window);
        SetShortcut(window, "Ctrl+Shift+F11");
        SettingsHarness.Done(window);
        Query(window, "2 days from now", DateTime.Now.Date.AddDays(2).ToString("d MMMM yyyy", CultureInfo.InvariantCulture));
    }
}
