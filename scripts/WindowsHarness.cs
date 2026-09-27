using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class WindowsHarness
{
    [StructLayout(LayoutKind.Sequential)] private struct WindowRectangle { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] private static extern bool GetClientRect(IntPtr window, out WindowRectangle rectangle);
    [DllImport("user32.dll", SetLastError=true)] private static extern bool RedrawWindow(IntPtr window, IntPtr area, IntPtr region, uint flags);
    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] private static extern bool GetLayeredWindowAttributes(IntPtr window, IntPtr color, out byte alpha, out uint flags);
    [DllImport("user32.dll")] private static extern bool SystemParametersInfoW(uint action, uint parameter, out bool enabled, uint flags);
    [DllImport("user32.dll")] private static extern int GetWindowRgn(IntPtr window, IntPtr region);
    [DllImport("gdi32.dll")] private static extern IntPtr CreateRectRgn(int left, int top, int right, int bottom);
    [DllImport("gdi32.dll")] private static extern bool PtInRegion(IntPtr region, int x, int y);
    [DllImport("gdi32.dll")] private static extern bool DeleteObject(IntPtr item);
    [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr process, uint flags);
    [StructLayout(LayoutKind.Sequential)] private struct ThreadGuiState {
        public uint Size, Flags;
        public IntPtr Active, Focus, Capture, MenuOwner, MoveSize, Caret;
        public int Left, Top, Right, Bottom;
    }
    private delegate bool WindowCallback(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] private static extern bool EnumWindows(WindowCallback callback, IntPtr parameter);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window, int identifier);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)] private static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr word, string text);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")] private static extern IntPtr ReadMessage(IntPtr window, uint message, IntPtr word, StringBuilder text);
    [DllImport("user32.dll", EntryPoint="SendMessageW")] private static extern IntPtr NumericMessage(IntPtr window, uint message, IntPtr word, IntPtr data);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr window, uint message, IntPtr word, IntPtr data);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetWindowTextW(IntPtr window, StringBuilder text, int maximum);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] private static extern bool GetGUIThreadInfo(uint threadId, ref ThreadGuiState state);

    public static IntPtr FindWindow(int processId)
    {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, parameter) => {
            GetWindowThreadProcessId(window, out uint owner);
            if (owner == processId && GetDlgItem(window, 100) != IntPtr.Zero) { found = window; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static string Text(IntPtr window)
    {
        var text = new StringBuilder(8192);
        ReadMessage(window, 0x000D, new IntPtr(text.Capacity), text);
        return text.ToString();
    }

    public static void Query(IntPtr window, string query) {
        if (SendMessageW(GetDlgItem(window, 100), 0x000C, IntPtr.Zero, query) == IntPtr.Zero)
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Could not set test query");
    }
    public static int Count(IntPtr window) { return NumericMessage(GetDlgItem(window, 101), 0x018B, IntPtr.Zero, IntPtr.Zero).ToInt32(); }
    public static string First(IntPtr window) { return Row(window, 0); }
    public static string Row(IntPtr window, int index)
    {
        if (Count(window) <= index || index < 0) return "";
        var text = new StringBuilder(8192);
        ReadMessage(GetDlgItem(window, 101), 0x0189, new IntPtr(index), text);
        return text.ToString();
    }
    public static void Enter(IntPtr window) { PostMessageW(GetDlgItem(window, 100), 0x0100, new IntPtr(13), IntPtr.Zero); }
    public static void Escape(IntPtr window) { PostMessageW(GetDlgItem(window, 100), 0x0100, new IntPtr(27), IntPtr.Zero); }
    public static void Close(IntPtr window) { PostMessageW(window, 0x0010, IntPtr.Zero, IntPtr.Zero); }
    public static void Show(IntPtr window) { PostMessageW(window, 0x8004, IntPtr.Zero, IntPtr.Zero); }

    public static byte Opacity(IntPtr window)
    {
        if (!GetLayeredWindowAttributes(window, IntPtr.Zero, out byte alpha, out uint flags))
            throw new InvalidOperationException("Could not read Core's transition opacity");
        return alpha;
    }

    public static void WarmWindow(IntPtr window)
    {
        Query(window, "@calc 1+1");
        WaitFor(() => First(window) == "2", "Warm-up query did not finish");
        Show(window);
        WaitFor(() => IsWindowVisible(window) && Opacity(window) == 255, "Warm-up show did not finish");
        // Reduced motion can reach full opacity before the first WM_PAINT warms native fonts.
        const uint RedrawImmediatelyWithChildren = 0x0185;
        if (!RedrawWindow(window, IntPtr.Zero, IntPtr.Zero, RedrawImmediatelyWithChildren))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Warm-up paint failed");
        Escape(window);
        WaitFor(() => !IsWindowVisible(window), "Warm-up hide did not finish");
    }

    public static bool VerifyStyling(IntPtr window, bool reducedMotion)
    {
        Query(window, "@calc 5+7");
        WaitFor(() => First(window) == "12", "Calculator row did not finish");
        uint dpi = GetDpiForWindow(window);
        GetClientRect(window, out WindowRectangle area);
        if (area.Right != (640 * dpi + 48) / 96 || area.Bottom != (258 * dpi + 48) / 96)
            throw new InvalidOperationException("Single-result layout did not shrink to the expected size");
        IntPtr region = CreateRectRgn(0, 0, 0, 0);
        try {
            if (GetWindowRgn(window, region) == 0 || PtInRegion(region, 0, 0) || !PtInRegion(region, area.Right / 2, area.Bottom / 2))
                throw new InvalidOperationException("Window does not have a rounded clipping region");
        } finally { DeleteObject(region); }

        bool animationsEnabled = !reducedMotion;
        bool observedFadeIn = false, observedFadeOut = false;
        Show(window);
        WaitFor(() => IsWindowVisible(window), "Show did not make the window visible");
        WaitFor(() => {
            byte alpha = Opacity(window);
            observedFadeIn |= alpha > 0 && alpha < 255;
            return alpha == 255;
        }, "Fade-in did not finish at full opacity");
        Escape(window);
        WaitFor(() => {
            byte alpha = Opacity(window);
            observedFadeOut |= alpha > 0 && alpha < 255;
            return !IsWindowVisible(window);
        }, "Escape did not finish its fade-out");
        if (Opacity(window) != 0) throw new InvalidOperationException("Hidden window retained visible opacity");
        if (animationsEnabled && (!observedFadeIn || !observedFadeOut))
            throw new InvalidOperationException("Animation preference is on but intermediate opacity was not observed");

        for (int cycle = 0; cycle < 20; cycle++) {
            Show(window);
            Thread.Sleep(25);
            Escape(window);
            Thread.Sleep(25);
            Show(window);
            WaitFor(() => IsWindowVisible(window) && Opacity(window) == 255, "Reversed fade did not finish visible");
            Escape(window);
            WaitFor(() => !IsWindowVisible(window), "Reversed fade did not finish hidden");
        }
        Query(window, "@");
        WaitFor(() => Count(window) == 8, "Command rows did not render");
        GetClientRect(window, out area);
        if (area.Bottom != (566 * dpi + 48) / 96) throw new InvalidOperationException("Multi-result height did not grow");
        PostMessageW(GetDlgItem(window, 100), 0x0100, new IntPtr(40), IntPtr.Zero);
        WaitFor(() => NumericMessage(GetDlgItem(window, 101), 0x0188, IntPtr.Zero, IntPtr.Zero).ToInt32() == 1, "Arrow-key selection failed");
        return animationsEnabled;
    }

    public static double[] MeasureQueries(IntPtr window, int count)
    {
        var durations = new double[count];
        for (int index = 0; index < count; index++) {
            var timer = Stopwatch.StartNew();
            Query(window, "@calc " + index + "+17");
            string expected = (index + 17).ToString();
            WaitFor(() => First(window) == expected, "Stress query did not finish");
            durations[index] = timer.Elapsed.TotalMilliseconds;
        }
        return durations;
    }

    private static bool MenuIsOpen(IntPtr window)
    {
        uint threadId = GetWindowThreadProcessId(window, out uint processId);
        var state = new ThreadGuiState { Size = (uint)Marshal.SizeOf<ThreadGuiState>() };
        if (!GetGUIThreadInfo(threadId, ref state)) throw new InvalidOperationException("Could not inspect test window's GUI state");
        return (state.Flags & 4) != 0;
    }

    public static IntPtr FocusedControl(IntPtr window)
    {
        uint threadId = GetWindowThreadProcessId(window, out uint processId);
        var state = new ThreadGuiState { Size = (uint)Marshal.SizeOf<ThreadGuiState>() };
        if (!GetGUIThreadInfo(threadId, ref state)) throw new InvalidOperationException("Could not inspect test focus");
        return state.Focus;
    }

    private static void VerifyMenuDoesNotBlockSearch(IntPtr window)
    {
        PostMessageW(window, 0x8003, IntPtr.Zero, new IntPtr(0x0205));
        WaitFor(() => MenuIsOpen(window), "Tray menu did not open");
        try {
            Query(window, "@calc 901+2");
            WaitFor(() => First(window) == "903", "Search completion was blocked by the tray menu");
        } finally {
            PostMessageW(window, 0x001F, IntPtr.Zero, IntPtr.Zero);
        }
        WaitFor(() => !MenuIsOpen(window), "Tray menu did not close");
    }

    public static void WaitFor(Func<bool> condition, string description)
    {
        var elapsed = Stopwatch.StartNew();
        while (!condition()) {
            if (elapsed.ElapsedMilliseconds > 5000) throw new TimeoutException(description);
            Thread.Sleep(2);
        }
    }

    public static void Verify(IntPtr window)
    {
        Query(window, "");
        WaitFor(() => Count(window) > 0, "No applications were discovered in the test account's Start Menu");
        string applicationName = First(window);
        Query(window, "@app " + applicationName);
        WaitFor(() => First(window) == applicationName, "Explicit app search did not preserve the exact result");
        Enter(window);
        WaitFor(() => Text(GetDlgItem(window, 102)).StartsWith("Verified action"), "Application action was not accepted");
        Query(window, "@calc 2+2");
        WaitFor(() => First(window) == "4", "Calculator did not display 4");
        Enter(window);
        WaitFor(() => Text(GetDlgItem(window, 102)).StartsWith("Verified action"), "Calculator action was not accepted");
        Query(window, "@calc 1/0");
        WaitFor(() => Count(window) == 0 && Text(GetDlgItem(window, 102)).Contains("divide by zero"), "Division error did not clear old results");
        Query(window, "@cal");
        WaitFor(() => First(window) == "@calc", "Command hint missing");
        Enter(window);
        WaitFor(() => Text(GetDlgItem(window, 100)) == "@calc ", "Hint did not complete the command");
        Query(window, "@web Rust & Windows");
        WaitFor(() => First(window) == "Search the web for Rust & Windows", "Web query payload changed");
        for (int index = 0; index < 100; index++) Query(window, "@calc " + index + "+1");
        Enter(window);
        WaitFor(() => First(window) == "100" && Text(GetDlgItem(window, 102)).StartsWith("Verified action"), "Latest-query acceptance failed");
        Query(window, "@unknown");
        Enter(window);
        WaitFor(() => Count(window) == 0 && Text(GetDlgItem(window, 102)).StartsWith("Unknown command"), "Invalid query retained a stale action");
        VerifyTimeConversions(window);
        VerifyMenuDoesNotBlockSearch(window);
        Escape(window);
        WaitFor(() => !IsWindowVisible(window), "Escape did not hide Core");
    }

    private static void VerifyTimeConversions(IntPtr window)
    {
        Query(window, "9pm et to uk");
        WaitFor(() => First(window).EndsWith("UK · next day") && Text(GetDlgItem(window, 102)) == "Enter to copy", "Implicit time conversion did not render");
        Query(window, "9pm et to uk on 2026-03-10");
        Enter(window);
        WaitFor(() => First(window) == "1:00 am UK · next day" && Text(GetDlgItem(window, 102)).StartsWith("Verified action"), "Dated conversion or immediate Enter failed");
        Query(window, "@time 21:00 et to uk on 2026-07-10");
        WaitFor(() => First(window) == "2:00 am UK · next day", "Explicit time command did not render");
        Query(window, "2:30am et to uk on 2026-03-08");
        Enter(window);
        WaitFor(() => Count(window) == 0 && Text(GetDlgItem(window, 102)).Contains("skipped"), "Nonexistent time retained a stale copy action");
        Query(window, "1:30am et to uk on 2026-11-01");
        WaitFor(() => Count(window) == 0 && Text(GetDlgItem(window, 102)).Contains("occurs twice"), "Ambiguous time was silently resolved");
        Query(window, "9pm et to uk on 2026-02-29");
        WaitFor(() => Count(window) == 0 && Text(GetDlgItem(window, 102)).StartsWith("Use a valid date"), "Invalid calendar date was accepted");
        Query(window, "9pm et to nowhere");
        WaitFor(() => Count(window) == 0 && Text(GetDlgItem(window, 102)).StartsWith("Unknown zone"), "Unknown time zone was accepted");
    }
}
