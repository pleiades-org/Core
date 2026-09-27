using System;
using System.IO;
using System.Runtime.InteropServices;

public static class SettingsHarness
{
    [StructLayout(LayoutKind.Sequential)] private struct Rectangle { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] private struct MonitorInfo { public int Size; public Rectangle Monitor, Work; public uint Flags; }
    [DllImport("user32.dll")] private static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr word, IntPtr data);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")] private static extern IntPtr TextMessage(IntPtr window, uint message, IntPtr word, string text);
    [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr window, out Rectangle rectangle);
    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] private static extern IntPtr MonitorFromWindow(IntPtr window, uint flags);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern bool GetMonitorInfoW(IntPtr monitor, ref MonitorInfo info);
    [DllImport("user32.dll")] private static extern int GetWindowRgn(IntPtr window, IntPtr region);
    [DllImport("gdi32.dll")] private static extern IntPtr CreateRectRgn(int left, int top, int right, int bottom);
    [DllImport("gdi32.dll")] private static extern bool PtInRegion(IntPtr region, int x, int y);
    [DllImport("gdi32.dll")] private static extern bool DeleteObject(IntPtr region);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern IntPtr GetPropW(IntPtr window, string name);
    [DllImport("user32.dll")] private static extern int GetDlgCtrlID(IntPtr window);
    [DllImport("user32.dll")] private static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    private static readonly IntPtr PerMonitorAwareV2 = new IntPtr(-4);
    private static int fence;

    public static void Flush(IntPtr window) {
        int expected = ++fence;
        WindowsHarness.PostMessageW(window, 0x800A, new IntPtr(expected), IntPtr.Zero);
        WindowsHarness.WaitFor(() => GetPropW(window, "Core.Test.Fence").ToInt64() == expected, "Core did not finish queued UI work");
    }

    public static void Click(IntPtr window, int identifier) {
        WindowsHarness.PostMessageW(window, 0x0111, new IntPtr(identifier), WindowsHarness.GetDlgItem(window, identifier));
        Flush(window);
    }
    // Picks a dropdown item as a person does: select it (CB_SETCURSEL), then CBN_SELCHANGE.
    public static void Choose(IntPtr window, int identifier, int index) {
        IntPtr control = WindowsHarness.GetDlgItem(window, identifier);
        if (SendMessageW(control, 0x014E, new IntPtr(index), IntPtr.Zero).ToInt64() != index)
            throw new InvalidOperationException("Dropdown " + identifier + " has no item " + index);
        WindowsHarness.PostMessageW(window, 0x0111, new IntPtr(identifier | (1 << 16)), control);
        Flush(window);
    }
    public static void SetColor(IntPtr window, string color) {
        if (TextMessage(WindowsHarness.GetDlgItem(window, 200), 0x000C, IntPtr.Zero, color) == IntPtr.Zero)
            throw new InvalidOperationException("Could not enter a background color");
        Flush(window);
    }
    public static bool IsOpen(IntPtr window) { return WindowsHarness.IsWindowVisible(WindowsHarness.GetDlgItem(window, 221)); }
    public static void Open(IntPtr window) {
        Click(window, 104);
        WindowsHarness.WaitFor(() => IsOpen(window), "Settings page did not open from its button");
    }
    public static void WaitSaved(IntPtr window) {
        WindowsHarness.WaitFor(() => WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 222)).StartsWith("All changes saved"), "Autosave did not finish");
        Flush(window);
    }
    public static void Done(IntPtr window) {
        Click(window, 221);
        WindowsHarness.WaitFor(() => !IsOpen(window), "Done did not return to search: " + WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 222)));
        Flush(window);
    }

    public static void CheckSidebar(IntPtr window, bool behaviour) {
        Flush(window);
        GetWindowRect(window, out Rectangle parent);
        GetWindowRect(WindowsHarness.GetDlgItem(window, 230), out Rectangle appearance);
        GetWindowRect(WindowsHarness.GetDlgItem(window, 231), out Rectangle behavior);
        GetWindowRect(WindowsHarness.GetDlgItem(window, behaviour ? 240 : 200), out Rectangle editor);
        if (appearance.Left != behavior.Left || appearance.Bottom > behavior.Top || appearance.Right >= editor.Left)
            throw new InvalidOperationException("Settings categories are not in a left sidebar beside their controls");
        if (WindowsHarness.IsWindowVisible(WindowsHarness.GetDlgItem(window, behaviour ? 200 : 240)))
            throw new InvalidOperationException("Inactive settings category is still visible");
        int[] identifiers = behaviour ? new[] {230, 231, 240, 241, 242, 243, 247, 248, 256, 257, 222, 221} : new[] {230, 231, 200, 201, 202, 203, 210, 211, 212, 213, 214, 215, 216, 251, 254, 222, 221};
        foreach (int identifier in identifiers) {
            IntPtr control = WindowsHarness.GetDlgItem(window, identifier);
            GetWindowRect(control, out Rectangle bounds);
            if (!WindowsHarness.IsWindowVisible(control) || bounds.Left < parent.Left || bounds.Right > parent.Right || bounds.Top < parent.Top || bounds.Bottom > parent.Bottom)
                throw new InvalidOperationException("Settings control is hidden or clipped: " + identifier);
        }
    }

    public static void CheckPosition(IntPtr window, int position) {
        // A result control can change before its parent finishes the same layout message.
        Flush(window);
        if (!GetWindowRect(window, out Rectangle bounds)) throw new InvalidOperationException("Cannot read Core bounds");
        var monitor = new MonitorInfo { Size = Marshal.SizeOf<MonitorInfo>() };
        if (!GetMonitorInfoW(MonitorFromWindow(window, 2), ref monitor)) throw new InvalidOperationException("Cannot read monitor work area");
        int width = bounds.Right - bounds.Left, height = bounds.Bottom - bounds.Top;
        int left = position == 3 || position == 5 ? monitor.Work.Left : position == 4 || position == 6 ? monitor.Work.Right - width : monitor.Work.Left + (monitor.Work.Right - monitor.Work.Left - width) / 2;
        int top = position == 1 ? monitor.Work.Top : position == 2 || position >= 5 ? monitor.Work.Bottom - height : monitor.Work.Top + (monitor.Work.Bottom - monitor.Work.Top - height) / 2;
        if (bounds.Left != left || bounds.Top != top) throw new InvalidOperationException("Wrong anchor for position " + position + ": " + bounds.Left + "," + bounds.Top + " expected " + left + "," + top);
        bool[] square = { position == 1 || position == 3 || position == 5, position == 1 || position == 4 || position == 6, position == 2 || position == 3 || position >= 5, position == 2 || position == 4 || position >= 5 };
        IntPtr region = CreateRectRgn(0, 0, 0, 0);
        try {
            if (GetWindowRgn(window, region) == 0) throw new InvalidOperationException("Missing window clipping region");
            int[] horizontal = { 0, width - 1, 0, width - 1 }, vertical = { 0, 0, height - 1, height - 1 };
            for (int corner = 0; corner < 4; corner++) {
                if (PtInRegion(region, horizontal[corner], vertical[corner]) != square[corner])
                    throw new InvalidOperationException("Incorrect corner " + corner + " for position " + position);
            }
        } finally { DeleteObject(region); }
    }

    /// Moves a trackbar and reports the release, as a mouse or keyboard user would.
    public static void Slide(IntPtr window, int identifier, int value) {
        IntPtr slider = WindowsHarness.GetDlgItem(window, identifier);
        SendMessageW(slider, 0x0405, new IntPtr(1), new IntPtr(value)); // TBM_SETPOS
        WindowsHarness.PostMessageW(window, 0x0114, new IntPtr(8), slider); // WM_HSCROLL, TB_ENDTRACK
        Flush(window);
    }

    /// Bottom-right placement sits `spacing` logical pixels inside the physical screen edges.
    /// Spaced windows float, so every corner is either rounded or (radius 0) square.
    public static void CheckSpacing(IntPtr window, int spacing, bool rounded) {
        Flush(window);
        // Compare physical pixels: a DPI-virtualized harness would see a scaled gap.
        IntPtr previous = SetThreadDpiAwarenessContext(PerMonitorAwareV2);
        try { CheckPhysicalSpacing(window, spacing, rounded); }
        finally { SetThreadDpiAwarenessContext(previous); }
    }

    private static void CheckPhysicalSpacing(IntPtr window, int spacing, bool rounded) {
        if (!GetWindowRect(window, out Rectangle bounds)) throw new InvalidOperationException("Cannot read Core bounds");
        var monitor = new MonitorInfo { Size = Marshal.SizeOf<MonitorInfo>() };
        if (!GetMonitorInfoW(MonitorFromWindow(window, 2), ref monitor)) throw new InvalidOperationException("Cannot read monitor bounds");
        int gap = (spacing * (int)GetDpiForWindow(window) + 48) / 96;
        if (monitor.Monitor.Right - bounds.Right != gap || monitor.Monitor.Bottom - bounds.Bottom != gap)
            throw new InvalidOperationException("Edge spacing " + spacing + " (gap " + gap + ") placed Core at " + bounds.Right + "," + bounds.Bottom + " on a monitor ending at " + monitor.Monitor.Right + "," + monitor.Monitor.Bottom);
        int width = bounds.Right - bounds.Left, height = bounds.Bottom - bounds.Top;
        IntPtr region = CreateRectRgn(0, 0, 0, 0);
        try {
            if (GetWindowRgn(window, region) == 0) throw new InvalidOperationException("Missing window clipping region");
            int[] horizontal = { 0, width - 1, 0, width - 1 }, vertical = { 0, 0, height - 1, height - 1 };
            for (int corner = 0; corner < 4; corner++) {
                if (PtInRegion(region, horizontal[corner], vertical[corner]) == rounded)
                    throw new InvalidOperationException("Corner " + corner + " should be " + (rounded ? "rounded" : "square"));
            }
        } finally { DeleteObject(region); }
    }

    public static void CheckBackground(IntPtr window, string screenshot, uint expectedRgb) {
        Flush(window);
        // Captures come from the compositor, which can lag an inactive window's painting
        // (for example behind a full-screen app during background runs). Wait for the frame.
        uint rgb = 0;
        var timer = System.Diagnostics.Stopwatch.StartNew();
        do {
            rgb = BackgroundPixel(window, screenshot);
            if (rgb == expectedRgb) return;
            System.Threading.Thread.Sleep(20);
        } while (timer.ElapsedMilliseconds < 2000);
        throw new InvalidOperationException("Unexpected background: " + rgb.ToString("X6") + " expected " + expectedRgb.ToString("X6"));
    }

    private static uint BackgroundPixel(IntPtr window, string screenshot) {
        CaptureWindow.Save(window, screenshot);
        byte[] bitmap = File.ReadAllBytes(screenshot);
        int offset = BitConverter.ToInt32(bitmap, 10), width = BitConverter.ToInt32(bitmap, 18), height = BitConverter.ToInt32(bitmap, 22);
        int horizontal = (int)(10 * GetDpiForWindow(window) / 96), vertical = (int)(70 * GetDpiForWindow(window) / 96);
        int pixel = offset + ((height - 1 - vertical) * width + horizontal) * 4;
        return (uint)(bitmap[pixel + 2] << 16 | bitmap[pixel + 1] << 8 | bitmap[pixel]);
    }

    public static void Verify(IntPtr window, string directory, IntPtr process) {
        if (WindowsHarness.GetDlgItem(window, 200) != IntPtr.Zero) throw new InvalidOperationException("Settings controls were created before opening settings");
        WindowsHarness.Query(window, "@calc 7+8");
        WindowsHarness.WaitFor(() => WindowsHarness.First(window) == "15", "Calculator did not finish");
        WindowsHarness.Show(window);
        WindowsHarness.WaitFor(() => WindowsHarness.IsWindowVisible(window), "Core did not show");
        Open(window);
        CheckSidebar(window, false);
        CheckPosition(window, 0);
        CheckBackground(window, Path.Combine(directory, "settings-black.bmp"), 0);
        WindowsHarness.PostMessageW(WindowsHarness.GetDlgItem(window, 200), 0x0100, new IntPtr(9), IntPtr.Zero);
        Flush(window);
        if (WindowsHarness.FocusedControl(window) != WindowsHarness.GetDlgItem(window, 201))
            throw new InvalidOperationException("Tab focused control " + GetDlgCtrlID(WindowsHarness.FocusedControl(window)) + " instead of the next settings control");
        for (int position = 0; position < 7; position++) {
            if (!IsOpen(window)) Open(window);
            SetColor(window, "#123456");
            Click(window, 210 + position);
            CheckPosition(window, position);
            WaitSaved(window);
            if (!IsOpen(window)) throw new InvalidOperationException("Autosave unexpectedly closed settings");
            Done(window);
            WindowsHarness.Query(window, "@calc 7+8");
            WindowsHarness.WaitFor(() => WindowsHarness.First(window) == "15", "Small result layout missing");
            CheckPosition(window, position);
            WindowsHarness.Query(window, "@");
            WindowsHarness.WaitFor(() => WindowsHarness.Count(window) == 8, "Large result layout missing (eight command hints)");
            CheckPosition(window, position);
        }
        Open(window);
        Click(window, 214);
        Done(window);
        Open(window);
        CheckBackground(window, Path.Combine(directory, "settings-right.bmp"), 0x123456);
        Click(window, 203);
        WindowsHarness.PostMessageW(window, 0x8007, IntPtr.Zero, IntPtr.Zero);
        Flush(window);
        if (WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 200)) != "#F5F5F5")
            throw new InvalidOperationException("Opening settings again discarded the active preview");
        CheckBackground(window, Path.Combine(directory, "settings-light.bmp"), 0xF5F5F5);
        Click(window, 213);
        Done(window);
        CheckPosition(window, 3);
        CheckBackground(window, Path.Combine(directory, "settings-done.bmp"), 0xF5F5F5);
        Open(window);
        uint initialGdi = WindowsHarness.GetGuiResources(process, 0);
        for (int change = 0; change < 30; change++) {
            SetColor(window, change % 2 == 0 ? "#F5F5F5" : "#123456");
            Click(window, 210 + change % 5);
        }
        uint finalGdi = WindowsHarness.GetGuiResources(process, 0);
        if (finalGdi > initialGdi + 2) throw new InvalidOperationException("Appearance previews leaked GDI objects: " + initialGdi + " to " + finalGdi);
        Click(window, 214);
        WaitSaved(window);
        SetColor(window, "#wrong");
        Click(window, 221);
        if (!IsOpen(window) || !WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 222)).Contains("six-digit"))
            throw new InvalidOperationException("Invalid color was not rejected with an explanation");
        WindowsHarness.PostMessageW(WindowsHarness.GetDlgItem(window, 200), 0x0100, new IntPtr(27), IntPtr.Zero);
        WindowsHarness.WaitFor(() => !IsOpen(window), "Escape did not leave invalid settings");
        CheckPosition(window, 4);
        CheckBackground(window, Path.Combine(directory, "settings-invalid-dismissed.bmp"), 0x123456);
    }
}
