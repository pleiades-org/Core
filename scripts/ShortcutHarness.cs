using System;
using System.Runtime.InteropServices;
using System.Threading;

public static class ShortcutHarness
{
    [StructLayout(LayoutKind.Sequential)] private struct Keyboard { public ushort Key, Scan; public uint Flags, Time; public UIntPtr Extra; }
    [StructLayout(LayoutKind.Explicit, Size=32)] private struct InputUnion { [FieldOffset(0)] public Keyboard Keyboard; }
    [StructLayout(LayoutKind.Sequential)] private struct Input { public uint Type; public InputUnion Data; }
    [StructLayout(LayoutKind.Sequential)] private struct Message { public IntPtr Window; public uint Id; public UIntPtr Word; public IntPtr Data; public uint Time; public int X, Y; public uint Private; }
    [DllImport("user32.dll")] private static extern uint SendInput(uint count, Input[] inputs, int size);
    [DllImport("user32.dll")] private static extern short GetAsyncKeyState(int key);
    [DllImport("user32.dll")] private static extern bool RegisterHotKey(IntPtr window, int id, uint modifiers, uint key);
    [DllImport("user32.dll")] private static extern bool UnregisterHotKey(IntPtr window, int id);
    [DllImport("user32.dll")] private static extern int GetMessageW(out Message message, IntPtr window, uint minimum, uint maximum);
    [DllImport("user32.dll")] private static extern bool PostThreadMessageW(uint thread, uint message, UIntPtr word, IntPtr data);
    [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
    private static int combinations;
    private static Input Key(ushort key, bool up) { return new Input { Type=1, Data=new InputUnion { Keyboard=new Keyboard { Key=key, Flags=(up ? 2u : 0u) | (key == 0x5B || key == 0x5C ? 1u : 0u), Extra=new UIntPtr(0x54455354) } } }; }
    private static void Send(params Input[] inputs) { if (SendInput((uint)inputs.Length, inputs, Marshal.SizeOf<Input>()) != inputs.Length) throw new InvalidOperationException("Windows rejected shortcut test input"); }
    private static void Chord(bool batched, params ushort[] keys) {
        var inputs = new Input[keys.Length * 2];
        for (int index = 0; index < keys.Length; index++) { inputs[index] = Key(keys[index], false); inputs[inputs.Length - index - 1] = Key(keys[index], true); }
        if (batched) Send(inputs); else foreach (var input in inputs) { Send(input); Thread.Sleep(20); }
    }
    private static void Hidden(IntPtr window) { WindowsHarness.Escape(window); WindowsHarness.WaitFor(() => !WindowsHarness.IsWindowVisible(window), "Core did not hide before shortcut test"); }
    private static void SetBinding(IntPtr window, string shortcut) {
        WindowsHarness.Show(window);
        WindowsHarness.WaitFor(() => WindowsHarness.IsWindowVisible(window), "Core did not show for settings");
        SettingsHarness.Open(window); SettingsHarness.Click(window, 231); ExtensionsHarness.SetShortcut(window, shortcut); SettingsHarness.Done(window);
        Hidden(window);
    }
    public static void Verify(IntPtr window) {
        var ready = new ManualResetEventSlim();
        uint threadId = 0;
        Exception failure = null;
        var receiver = new Thread(() => {
            threadId = GetCurrentThreadId();
            if (!RegisterHotKey(IntPtr.Zero, 55, 0x400F, 0x79)) { failure = new InvalidOperationException("Test-only Win+Ctrl+Alt+Shift+F10 binding unavailable"); ready.Set(); return; }
            ready.Set();
            try { while (GetMessageW(out Message message, IntPtr.Zero, 0, 0) > 0) { if (message.Id == 0x0312) Interlocked.Increment(ref combinations); } }
            finally { UnregisterHotKey(IntPtr.Zero, 55); }
        });
        receiver.Start(); ready.Wait();
        if (failure != null) { receiver.Join(); throw failure; }
        try {
            Chord(true, 0x11, 0x10, 0x7A);
            WindowsHarness.WaitFor(() => WindowsHarness.IsWindowVisible(window), "Configured Ctrl+Shift+F11 did not open Core");
            Hidden(window);
            SetBinding(window, "Win");
            foreach (ushort key in new ushort[] {0x5B, 0x5C}) {
                Chord(true, key);
                WindowsHarness.WaitFor(() => WindowsHarness.IsWindowVisible(window), "Windows-key tap did not open Core");
                SettingsHarness.Flush(window);
                Chord(true, key);
                WindowsHarness.WaitFor(() => !WindowsHarness.IsWindowVisible(window), "Windows-key tap did not toggle Core closed");
            }
            foreach (bool batched in new[] {false, true}) {
                int previous = Volatile.Read(ref combinations);
                Chord(batched, 0x5B, 0x11, 0x12, 0x10, 0x79);
                WindowsHarness.WaitFor(() => Volatile.Read(ref combinations) == previous + 1, "Windows combination did not survive interception; batched=" + batched);
                if (WindowsHarness.IsWindowVisible(window)) throw new InvalidOperationException("A Windows combination incorrectly opened Core");
                foreach (int key in new[] {0x5B, 0x5C, 0x11, 0x12, 0x10}) if (GetAsyncKeyState(key) < 0) throw new InvalidOperationException("Modifier stuck after replay: " + key);
            }
            SetBinding(window, "Ctrl+Shift+F11");
            Chord(true, 0x11, 0x10, 0x7A);
            WindowsHarness.WaitFor(() => WindowsHarness.IsWindowVisible(window), "Restored shortcut failed");
            // Deliberately conflict with the registered fixture hotkey; saving must preserve the old binding.
            SettingsHarness.Open(window); SettingsHarness.Click(window, 231);
            ExtensionsHarness.SetShortcut(window, "Win+Ctrl+Alt+Shift+F10");
            WindowsHarness.WaitFor(() => WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 222)).Contains("unavailable"), "Autosave did not report the shortcut conflict");
            if (!SettingsHarness.IsOpen(window) || !WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 222)).Contains("unavailable")) throw new InvalidOperationException("Shortcut conflict was not rejected");
            WindowsHarness.PostMessageW(WindowsHarness.GetDlgItem(window, 240), 0x0100, new IntPtr(27), IntPtr.Zero);
            WindowsHarness.WaitFor(() => !SettingsHarness.IsOpen(window), "Escape did not leave the rejected shortcut edit");
            Hidden(window);
            Chord(true, 0x11, 0x10, 0x7A);
            WindowsHarness.WaitFor(() => WindowsHarness.IsWindowVisible(window), "Shortcut conflict destroyed the previous working binding");
        } finally {
            Send(Key(0x5B, true), Key(0x5C, true), Key(0x11, true), Key(0x12, true), Key(0x10, true), Key(0x79, true), Key(0x7A, true));
            PostThreadMessageW(threadId, 0x0012, UIntPtr.Zero, IntPtr.Zero);
            if (!receiver.Join(5000)) throw new InvalidOperationException("Shortcut receiver did not close");
            ready.Dispose();
        }
    }

    public static void VerifyRegistration(IntPtr window) {
        const int probeId = 56;
        if (RegisterHotKey(IntPtr.Zero, probeId, 0x4006, 0x7A)) {
            UnregisterHotKey(IntPtr.Zero, probeId);
            throw new InvalidOperationException("Core did not register Ctrl+Shift+F11");
        }
        SetBinding(window, "Win");
        if (!RegisterHotKey(IntPtr.Zero, probeId, 0x4006, 0x7A)) throw new InvalidOperationException("Switching to Windows-key mode did not release the old shortcut");
        UnregisterHotKey(IntPtr.Zero, probeId);
        SetBinding(window, "Ctrl+Shift+F11");
        if (!RegisterHotKey(IntPtr.Zero, probeId, 0x4006, 0x79)) throw new InvalidOperationException("Fixture shortcut unavailable");
        try {
            WindowsHarness.Show(window); SettingsHarness.Flush(window);
            SettingsHarness.Open(window); SettingsHarness.Click(window, 231);
            ExtensionsHarness.SetShortcut(window, "Ctrl+Shift+F10");
            WindowsHarness.WaitFor(() => WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 222)).Contains("unavailable"), "Shortcut conflict was not reported");
            if (RegisterHotKey(IntPtr.Zero, probeId + 1, 0x4006, 0x7A)) {
                UnregisterHotKey(IntPtr.Zero, probeId + 1);
                throw new InvalidOperationException("Conflict released the previous working shortcut");
            }
            WindowsHarness.PostMessageW(WindowsHarness.GetDlgItem(window, 240), 0x0100, new IntPtr(27), IntPtr.Zero);
            SettingsHarness.Flush(window);
        } finally { UnregisterHotKey(IntPtr.Zero, probeId); }
    }
}
