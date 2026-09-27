using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;

public static class DismissalHarness
{
    private const uint PopupStyle = 0x80800000;
    private const uint ToolWindowStyle = 0x80;
    private const uint ForegroundChanged = 0x8006;

    [StructLayout(LayoutKind.Sequential)] private struct Point { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] private struct Rectangle { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] private struct MouseInput {
        public int X, Y;
        public uint Data, Flags, Time;
        public UIntPtr Extra;
    }
    [StructLayout(LayoutKind.Sequential)] private struct Input { public uint Type; public MouseInput Mouse; }
    [StructLayout(LayoutKind.Sequential)] private struct Message {
        public IntPtr Window;
        public uint Identifier;
        public UIntPtr Word;
        public IntPtr Data;
        public uint Time;
        public Point Position;
        public uint Private;
    }

    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    private static extern IntPtr CreateWindowExW(uint extendedStyle, string className, string title, uint style,
        int left, int top, int width, int height, IntPtr owner, IntPtr menu, IntPtr instance, IntPtr parameter);
    [DllImport("user32.dll")] private static extern bool DestroyWindow(IntPtr window);
    [DllImport("user32.dll")] private static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] private static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] private static extern IntPtr GetForegroundWindow();
    [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", SetLastError=true)] private static extern bool AttachThreadInput(uint source, uint destination, bool attach);
    [DllImport("user32.dll")] private static extern bool BringWindowToTop(IntPtr window);
    [DllImport("user32.dll")] private static extern bool GetCursorPos(out Point point);
    [DllImport("user32.dll")] private static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr window, out Rectangle rectangle);
    [DllImport("user32.dll")] private static extern IntPtr WindowFromPoint(Point point);
    [DllImport("user32.dll")] private static extern IntPtr GetAncestor(IntPtr window, uint flag);
    [DllImport("user32.dll")] private static extern bool SetWindowPos(IntPtr window, IntPtr order, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll", SetLastError=true)] private static extern uint SendInput(uint count, Input[] inputs, int size);
    [DllImport("user32.dll")] private static extern bool PeekMessageW(out Message message, IntPtr window, uint minimum, uint maximum, uint remove);
    [DllImport("user32.dll")] private static extern bool TranslateMessage(ref Message message);
    [DllImport("user32.dll")] private static extern IntPtr DispatchMessageW(ref Message message);
    [DllImport("user32.dll")] private static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr word, IntPtr data);
    [DllImport("user32.dll", SetLastError=true)] private static extern IntPtr SendMessageTimeoutW(IntPtr window, uint message, IntPtr word, IntPtr data, uint flags, uint timeout, out UIntPtr result);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern IntPtr GetPropW(IntPtr window, string name);
    private static int fence;

    private static IntPtr CreateTarget(IntPtr owner)
    {
        IntPtr window = CreateWindowExW(ToolWindowStyle, "BUTTON", "Core dismissal test", PopupStyle,
            20, 20, 230, 90, owner, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero);
        if (window == IntPtr.Zero) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
        ShowWindow(window, 4);
        return window;
    }

    private static void Pump()
    {
        while (PeekMessageW(out Message message, IntPtr.Zero, 0, 0, 1)) {
            TranslateMessage(ref message);
            DispatchMessageW(ref message);
        }
    }

    private static void WaitFor(Func<bool> condition, string description)
    {
        var timer = Stopwatch.StartNew();
        while (true) {
            Pump();
            if (condition()) return;
            if (timer.ElapsedMilliseconds > 5000) throw new TimeoutException(description);
            Thread.Sleep(2);
        }
    }

    public static void Activate(IntPtr window)
    {
        if (!SetForegroundWindow(window) && GetForegroundWindow() != window) {
            uint currentThread = GetCurrentThreadId();
            uint foregroundThread = GetWindowThreadProcessId(GetForegroundWindow(), out uint processId);
            if (currentThread != foregroundThread && AttachThreadInput(currentThread, foregroundThread, true)) {
                try {
                    BringWindowToTop(window);
                    SetForegroundWindow(window);
                    WaitFor(() => GetForegroundWindow() == window, "Attached test queue did not activate its target");
                } finally {
                    if (!AttachThreadInput(currentThread, foregroundThread, false))
                        throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Could not detach test input queue");
                }
            } else {
                ClickTarget(window);
            }
        }
        WaitFor(() => GetForegroundWindow() == window, "Test target did not receive foreground activation");
    }

    private static void ClickTarget(IntPtr window)
    {
        if (!GetCursorPos(out Point original) || !GetWindowRect(window, out Rectangle rectangle))
            throw new InvalidOperationException("Could not inspect the test click target");
        // Only a test-owned window is raised; verify the hit target before sending input.
        if (!SetWindowPos(window, new IntPtr(-1), 0, 0, 0, 0, 0x13))
            throw new InvalidOperationException("Could not raise test window");
        try {
            var target = new Point { X = rectangle.Left + 8, Y = rectangle.Top + 40 };
            IntPtr hit = WindowFromPoint(target);
            if (hit != window && GetAncestor(hit, 2) != window)
                throw new InvalidOperationException("Test click would hit an unrelated window: expected=" + window + ", hit=" + hit);
            WaitFor(() => {
                if (!SetCursorPos(target.X, target.Y) || !GetCursorPos(out Point actual))
                    throw new InvalidOperationException("Could not position test cursor");
                return actual.X == target.X && actual.Y == target.Y;
            }, "Test cursor did not reach its target");
            var inputs = new[] {
                new Input { Mouse = new MouseInput { Flags = 0x0002 } },
                new Input { Mouse = new MouseInput { Flags = 0x0004 } }
            };
            if (SendInput(2, inputs, Marshal.SizeOf<Input>()) != 2)
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Test click was rejected");
            try {
                WaitFor(() => GetForegroundWindow() == window, "Test click did not activate its target");
            } catch (TimeoutException) {
                GetCursorPos(out Point actual);
                throw new InvalidOperationException("Test click did not activate: target=" + window + ", foreground=" + GetForegroundWindow()
                    + ", cursor=" + actual.X + "," + actual.Y + ", targetPoint=" + target.X + "," + target.Y);
            }
        } finally {
            SetCursorPos(original.X, original.Y);
            SetWindowPos(window, new IntPtr(-2), 0, 0, 0, 0, 0x13);
        }
    }

    private static void Reopen(IntPtr window, string phase = "reopen")
    {
        // Posted commands cannot be dropped by a reentrant state borrow during another UI update.
        int expected = ++fence;
        WindowsHarness.Show(window);
        WindowsHarness.PostMessageW(window, 0x800A, new IntPtr(expected), IntPtr.Zero);
        WaitFor(() => GetPropW(window, "Core.Test.Fence").ToInt64() == expected, "Core did not finish the queued show request");
        Activate(window);
        try {
            WaitFor(() => WindowsHarness.IsWindowVisible(window) && WindowsHarness.Opacity(window) == 255
                && GetForegroundWindow() == window, "Core did not reopen focused at full opacity");
        } catch (TimeoutException) {
            throw new InvalidOperationException(phase + ": visible=" + WindowsHarness.IsWindowVisible(window)
                + ", alpha=" + WindowsHarness.Opacity(window) + ", foreground=" + GetForegroundWindow()
                + ", expected=" + window);
        }
    }

    private static void RequireVisibleFor(IntPtr window, int milliseconds)
    {
        var timer = Stopwatch.StartNew();
        while (timer.ElapsedMilliseconds < milliseconds) {
            Pump();
            if (!WindowsHarness.IsWindowVisible(window) || WindowsHarness.Opacity(window) != 255)
                throw new InvalidOperationException("Core dismissed while focus remained within its own windows");
            Thread.Sleep(2);
        }
    }

    public static double[] Verify(IntPtr window)
    {
        IntPtr outside = CreateTarget(IntPtr.Zero);
        string phase = "initial show";
        try {
            WindowsHarness.Query(window, "@calc 7+8");
            WaitFor(() => WindowsHarness.First(window) == "15", "Result did not become ready");
            Reopen(window, "initial show");
            // An unknown observer must not be allowed to dismiss the focused launcher.
            WindowsHarness.PostMessageW(window, ForegroundChanged, IntPtr.Zero, IntPtr.Zero);
            SendMessageW(WindowsHarness.GetDlgItem(window, 101), 0x0201, new IntPtr(1), new IntPtr((12 << 16) | 60));
            SendMessageW(WindowsHarness.GetDlgItem(window, 101), 0x0202, IntPtr.Zero, new IntPtr((12 << 16) | 60));
            if (WindowsHarness.FocusedControl(window) != WindowsHarness.GetDlgItem(window, 101))
                throw new InvalidOperationException("Result-list click did not move keyboard focus; actual=" + WindowsHarness.FocusedControl(window) + ", expected=" + WindowsHarness.GetDlgItem(window, 101) + ", foreground=" + GetForegroundWindow());
            RequireVisibleFor(window, 200);

            var timings = new double[12];
            for (int cycle = 0; cycle < timings.Length; cycle++) {
                phase = "outside activation cycle " + cycle;
                var timer = Stopwatch.StartNew();
                Activate(outside);
                WaitFor(() => !WindowsHarness.IsWindowVisible(window), "Core stayed visible after outside activation in cycle " + cycle);
                timings[cycle] = timer.Elapsed.TotalMilliseconds;
                if (WindowsHarness.Opacity(window) != 0) throw new InvalidOperationException("Dismissed Core retained visible opacity");
                if (GetForegroundWindow() == window) throw new InvalidOperationException("Core stole focus back after dismissal");
                phase = "reopen cycle " + cycle;
                Reopen(window, phase);
                if (WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 100)) != "@calc 7+8")
                    throw new InvalidOperationException("Reopening changed the input text: " + WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 100)));
                WaitFor(() => WindowsHarness.First(window) == "15", "Reopened query did not finish");
            }
            // Dismiss during an entrance transition, then immediately reopen.
            phase = "interrupted entrance";
            WindowsHarness.Escape(window);
            WaitFor(() => !WindowsHarness.IsWindowVisible(window), "Initial hide did not finish");
            WindowsHarness.Show(window);
            WaitFor(() => WindowsHarness.IsWindowVisible(window) && WindowsHarness.Opacity(window) > 0, "Entrance did not become visible");
            Activate(window);
            WaitFor(() => WindowsHarness.IsWindowVisible(window) && GetForegroundWindow() == window, "Entrance did not start");
            Activate(outside);
            Reopen(window, "interrupted entrance");
            RequireVisibleFor(window, 200);
            return timings;
        } catch (Exception error) {
            throw new InvalidOperationException(phase + ": " + error.Message, error);
        } finally {
            DestroyWindow(outside);
        }
    }
}
