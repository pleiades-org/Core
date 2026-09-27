using System;
using System.IO;
using System.Runtime.InteropServices;

public static class ControlsHarness
{
    [StructLayout(LayoutKind.Sequential)] private struct Rectangle { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] private struct Point { public int X, Y; }
    [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr window, out Rectangle rectangle);
    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern IntPtr GetPropW(IntPtr window, string name);
    [DllImport("user32.dll")] private static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr word, IntPtr data);
    [DllImport("user32.dll")] private static extern bool SetCursorPos(int horizontal, int vertical);
    [DllImport("user32.dll")] private static extern bool GetCursorPos(out Point point);
    [DllImport("user32.dll")] private static extern IntPtr WindowFromPoint(Point point);
    [DllImport("user32.dll")] private static extern int GetDlgCtrlID(IntPtr window);
    [DllImport("user32.dll")] private static extern IntPtr GetTopWindow(IntPtr window);
    [DllImport("user32.dll")] private static extern IntPtr GetWindow(IntPtr window, uint command);
    [DllImport("user32.dll")] private static extern bool RedrawWindow(IntPtr window, IntPtr area, IntPtr region, uint flags);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetClassNameW(IntPtr window, System.Text.StringBuilder name, int capacity);

    private static void CheckContained(IntPtr window, int identifier) {
        IntPtr control = WindowsHarness.GetDlgItem(window, identifier);
        if (!WindowsHarness.IsWindowVisible(control)) throw new InvalidOperationException("Control " + identifier + " is hidden");
        if (!GetWindowRect(window, out Rectangle parent) || !GetWindowRect(control, out Rectangle child))
            throw new InvalidOperationException("Could not measure control " + identifier);
        if (child.Left < parent.Left || child.Right > parent.Right || child.Top < parent.Top || child.Bottom > parent.Bottom)
            throw new InvalidOperationException("Control " + identifier + " is outside Core: relative bounds " +
                (child.Left-parent.Left) + "," + (child.Top-parent.Top) + ".." + (child.Right-parent.Left) + "," +
                (child.Bottom-parent.Top) + "; Core size " + (parent.Right-parent.Left) + "x" + (parent.Bottom-parent.Top));
    }

    public static void VerifyLayout(IntPtr window, string directory) {
        // Nothing typed shows recently used apps as a grid, or the first applications without any.
        WindowsHarness.WaitFor(() => WindowsHarness.Count(window) > 0, "Default applications did not load");
        WindowsHarness.WaitFor(() => WindowsHarness.IsWindowVisible(window) && WindowsHarness.Opacity(window) == 255, "Initial display did not finish");
        SettingsHarness.Flush(window);
        if (WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 100)) != "") throw new InvalidOperationException("Default query should be empty");
        CheckContained(window, 105);
        CaptureWindow.Save(window, Path.Combine(directory, "controls-default.bmp"));
        VerifyGridKeys(window);
        uint originalDpi = GetDpiForWindow(window);
        try {
            // Exercise Core's existing DPI update message without changing desktop settings.
            WindowsHarness.PostMessageW(window, 0x8009, new IntPtr(384), IntPtr.Zero);
            SettingsHarness.Flush(window);
            // The default view keeps its footer and buttons on screen; grid rows that cannot fit
            // are left out rather than cut off.
            CaptureWindow.Save(window, Path.Combine(directory, "controls-constrained-default.bmp"));
            CheckContained(window, 104);
            CheckContained(window, 105);
            CheckContained(window, 102);
            // Eight rows of results that do not fit must still be reachable by keyboard.
            WindowsHarness.Query(window, "@app a");
            WindowsHarness.WaitFor(() => WindowsHarness.Count(window) == 8, "Eight results for '@app a' did not load");
            SettingsHarness.Flush(window);
            CaptureWindow.Save(window, Path.Combine(directory, "controls-constrained.bmp"));
            CheckContained(window, 104);
            CheckContained(window, 105);
            CheckContained(window, 102);
            CheckContained(window, 101);
            if (WindowsHarness.Count(window) != 8) throw new InvalidOperationException("Constrained layout discarded results");
            for (int step = 0; step < 7; step++) WindowsHarness.PostMessageW(WindowsHarness.GetDlgItem(window, 100), 0x0100, new IntPtr(40), IntPtr.Zero);
            SettingsHarness.Flush(window);
            IntPtr list = WindowsHarness.GetDlgItem(window, 101);
            if (SendMessageW(list, 0x0188, IntPtr.Zero, IntPtr.Zero).ToInt32() != 7 || SendMessageW(list, 0x018E, IntPtr.Zero, IntPtr.Zero).ToInt32() == 0)
                throw new InvalidOperationException("Clipped results cannot be reached with the keyboard");
            SettingsHarness.Click(window, 105);
            foreach (int identifier in new[] {106, 107, 108, 109}) CheckContained(window, identifier);
            SettingsHarness.Click(window, 105);
        } finally {
            WindowsHarness.PostMessageW(window, 0x8009, new IntPtr(originalDpi), IntPtr.Zero);
            WindowsHarness.Query(window, "");
            SettingsHarness.Flush(window);
        }
        SettingsHarness.Open(window);
        try {
            WindowsHarness.PostMessageW(window, 0x8009, new IntPtr(384), IntPtr.Zero);
            SettingsHarness.Flush(window);
            SettingsHarness.CheckSidebar(window, false);
            SettingsHarness.Click(window, 231);
            SettingsHarness.CheckSidebar(window, true);
            SettingsHarness.Click(window, 232);
            foreach (int identifier in new[] {230, 231, 232, 300, 301, 221}) CheckContained(window, identifier);
            SettingsHarness.Click(window, 230);
        } finally {
            WindowsHarness.PostMessageW(window, 0x8009, new IntPtr(originalDpi), IntPtr.Zero);
            SettingsHarness.Flush(window);
        }
        CaptureWindow.Save(window, Path.Combine(directory, "controls-position-boxes.bmp"));
        SettingsHarness.Click(window, 221);
        CheckContained(window, 105);
        WindowsHarness.Escape(window);
        WindowsHarness.WaitFor(() => !WindowsHarness.IsWindowVisible(window), "Core did not hide");
        WindowsHarness.Show(window);
        WindowsHarness.WaitFor(() => WindowsHarness.IsWindowVisible(window) && WindowsHarness.Opacity(window) == 255, "Core did not reopen");
        SettingsHarness.Flush(window);
        CheckContained(window, 105);
    }

    // With recently used apps as a grid (rows hidden), arrow keys move by tile and by row of six.
    private static void VerifyGridKeys(IntPtr window) {
        IntPtr list = WindowsHarness.GetDlgItem(window, 101);
        if (WindowsHarness.IsWindowVisible(list) || WindowsHarness.Count(window) < 8) return;
        Func<int> selected = () => SendMessageW(list, 0x0188, IntPtr.Zero, IntPtr.Zero).ToInt32();
        foreach (var step in new[] { (Key: 0x27, Expected: 1), (Key: 0x28, Expected: 7), (Key: 0x25, Expected: 6), (Key: 0x26, Expected: 0) }) {
            WindowsHarness.PostMessageW(WindowsHarness.GetDlgItem(window, 100), 0x0100, new IntPtr(step.Key), IntPtr.Zero);
            WindowsHarness.WaitFor(() => selected() == step.Expected, "Grid key " + step.Key + " selected " + selected() + ", not " + step.Expected);
        }
    }

    public static void VerifyHover(IntPtr window, string directory, IntPtr process) {
        DismissalHarness.Activate(window);
        SettingsHarness.Flush(window);
        if (!GetCursorPos(out Point originalPointer)) throw new InvalidOperationException("Could not read pointer position");
        try { VerifyHoverControls(window, directory, process); }
        finally { SetCursorPos(originalPointer.X, originalPointer.Y); }
    }

    private static void VerifyHoverControls(IntPtr window, string directory, IntPtr process) {
        CheckHover(window, 105, directory);
        SettingsHarness.Click(window, 105);
        SettingsHarness.Flush(window);
        CaptureWindow.Save(window, Path.Combine(directory, "controls-power-menu.bmp"));
        using (var log = new StreamWriter(Path.Combine(directory, "controls-z-order.txt"))) {
            for (IntPtr child = GetTopWindow(window); child != IntPtr.Zero; child = GetWindow(child, 2))
                log.WriteLine(GetDlgCtrlID(child) + ": " + WindowsHarness.IsWindowVisible(child));
        }
        foreach (int identifier in new[] {107, 108, 109}) {
            CheckContained(window, identifier);
            CheckHover(window, identifier, directory);
        }
        if (!RedrawWindow(window, IntPtr.Zero, IntPtr.Zero, 0x0181)) throw new InvalidOperationException("Could not paint complete popup");
        CaptureWindow.SaveVisible(window, Path.Combine(directory, "controls-power-menu-visible.bmp"));
        IntPtr restart = WindowsHarness.GetDlgItem(window, 108);
        MovePointer(restart);
        SendMessageW(restart, 0x0200, IntPtr.Zero, new IntPtr(0x00080008));
        WindowsHarness.WaitFor(() => GetPropW(restart, "Core.ButtonHovered") != IntPtr.Zero, "Restart did not hover");
        SettingsHarness.Click(window, 105);
        if (GetPropW(restart, "Core.ButtonHovered") != IntPtr.Zero) throw new InvalidOperationException("Hidden power action retained hover state");
        uint initialGdi = WindowsHarness.GetGuiResources(process, 0);
        IntPtr power = WindowsHarness.GetDlgItem(window, 105);
        for (int cycle = 0; cycle < 50; cycle++) {
            SendMessageW(power, 0x0200, IntPtr.Zero, new IntPtr(0x00080008));
            SendMessageW(power, 0x02A3, IntPtr.Zero, IntPtr.Zero);
        }
        SettingsHarness.Flush(window);
        if (WindowsHarness.GetGuiResources(process, 0) > initialGdi) throw new InvalidOperationException("Hover leaked GDI objects");
    }

    private static void CheckHover(IntPtr window, int identifier, string directory) {
        IntPtr control = WindowsHarness.GetDlgItem(window, identifier);
        MovePointer(WindowsHarness.GetDlgItem(window, 100));
        WindowsHarness.WaitFor(() => GetPropW(control, "Core.ButtonHovered") == IntPtr.Zero, "Pointer leave did not clear hover for " + identifier);
        string beforePath = Path.Combine(directory, "controls-" + identifier + "-rest.bmp");
        SettingsHarness.Flush(window);
        if (!RedrawWindow(control, IntPtr.Zero, IntPtr.Zero, 0x0101)) throw new InvalidOperationException("Could not repaint resting icon");
        CaptureWindow.Save(control, beforePath);
        CaptureWindow.SaveVisible(control, Path.Combine(directory, "controls-" + identifier + "-rest-visible.bmp"));
        MovePointer(control);
        SettingsHarness.Flush(window);
        SendMessageW(control, 0x0200, IntPtr.Zero, new IntPtr(0x00080008));
        GetCursorPos(out Point hoverPoint);
        IntPtr hit = WindowFromPoint(hoverPoint);
        var hitClass = new System.Text.StringBuilder(256);
        GetClassNameW(hit, hitClass, hitClass.Capacity);
        GetWindowRect(control, out Rectangle targetBounds);
        WindowsHarness.WaitFor(() => GetPropW(control, "Core.ButtonHovered") != IntPtr.Zero, "Hover state missing for " + identifier + "; pointer hit control=" + GetDlgCtrlID(hit) + ", class=" + hitClass + ", expected window=" + control + ", actual=" + hit + ", cursor=" + hoverPoint.X + "," + hoverPoint.Y + ", target=" + targetBounds.Left + "," + targetBounds.Top + ".." + targetBounds.Right + "," + targetBounds.Bottom + ", visible=" + WindowsHarness.IsWindowVisible(window));
        string hoverPath = Path.Combine(directory, "controls-" + identifier + "-hover.bmp");
        SettingsHarness.Flush(window);
        if (!RedrawWindow(control, IntPtr.Zero, IntPtr.Zero, 0x0101)) throw new InvalidOperationException("Could not repaint hovered icon");
        CaptureWindow.Save(control, hoverPath);
        CaptureWindow.SaveVisible(control, Path.Combine(directory, "controls-" + identifier + "-hover-visible.bmp"));
        byte[] resting = File.ReadAllBytes(beforePath), hovered = File.ReadAllBytes(hoverPath);
        bool changed = false;
        for (int index = 54; index < resting.Length; index++) if (resting[index] != hovered[index]) { changed = true; break; }
        if (!changed) throw new InvalidOperationException("Hover did not change icon appearance for " + identifier + "; hovered=" + GetPropW(control, "Core.ButtonHovered") + "; pointer=" + hoverPoint.X + "," + hoverPoint.Y + "; hit=" + GetDlgCtrlID(hit));
        MovePointer(WindowsHarness.GetDlgItem(window, 100));
        WindowsHarness.WaitFor(() => GetPropW(control, "Core.ButtonHovered") == IntPtr.Zero, "Hover did not clear for " + identifier);
    }

    private static void MovePointer(IntPtr control) {
        if (!GetWindowRect(control, out Rectangle bounds)) throw new InvalidOperationException("Could not measure pointer target");
        int horizontal = (bounds.Left + bounds.Right) / 2, vertical = (bounds.Top + bounds.Bottom) / 2;
        WindowsHarness.WaitFor(() => {
            if (!SetCursorPos(horizontal, vertical) || !GetCursorPos(out Point position)) throw new InvalidOperationException("Could not move pointer over test control");
            return position.X == horizontal && position.Y == vertical;
        }, "Pointer did not reach test control " + GetDlgCtrlID(control));
    }
}
