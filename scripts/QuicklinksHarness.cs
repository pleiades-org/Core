using System;
using System.IO;
using System.Runtime.InteropServices;

public static class QuicklinksHarness
{
    [StructLayout(LayoutKind.Sequential)] private struct Rectangle { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr window, out Rectangle bounds);
    [StructLayout(LayoutKind.Sequential)] private struct Keyboard { public ushort Key, Scan; public uint Flags, Time; public UIntPtr Extra; }
    [StructLayout(LayoutKind.Explicit, Size=32)] private struct InputUnion { [FieldOffset(0)] public Keyboard Keyboard; }
    [StructLayout(LayoutKind.Sequential)] private struct Input { public uint Type; public InputUnion Data; }
    [DllImport("user32.dll")] private static extern uint SendInput(uint count, Input[] inputs, int size);
    [DllImport("user32.dll")] private static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr word, IntPtr data);
    [DllImport("user32.dll")] private static extern bool RedrawWindow(IntPtr window, IntPtr area, IntPtr region, uint flags);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")]
    private static extern IntPtr TextMessage(IntPtr window, uint message, IntPtr word, string text);

    public static void SetField(IntPtr window, int identifier, string text) {
        if (TextMessage(WindowsHarness.GetDlgItem(window, identifier), 0x000C, IntPtr.Zero, text) == IntPtr.Zero)
            throw new InvalidOperationException("Could not edit quicklink field " + identifier);
        SettingsHarness.Flush(window);
    }

    public static void Scroll(IntPtr window, int command) {
        WindowsHarness.PostMessageW(window, 0x0115, new IntPtr(command), WindowsHarness.GetDlgItem(window, 320));
        SettingsHarness.Flush(window);
    }

    public static void Query(IntPtr window, string query, string expected) {
        WindowsHarness.Query(window, query);
        WindowsHarness.WaitFor(() => WindowsHarness.First(window) == expected, "Quicklink query failed: " + query);
        SettingsHarness.Flush(window);
    }

    private static void CaptureTable(IntPtr window, string path) {
        SettingsHarness.Flush(window);
        if (!RedrawWindow(window, IntPtr.Zero, IntPtr.Zero, 0x0185)) throw new InvalidOperationException("Could not repaint quicklink table");
        CaptureWindow.Save(window, path);
        byte[] bitmap = File.ReadAllBytes(path);
        int width = BitConverter.ToInt32(bitmap, 18), height = BitConverter.ToInt32(bitmap, 22), offset = BitConverter.ToInt32(bitmap, 10);
        int brightPixels = 0;
        for (int row = 26 * width / 840; row < 58 * width / 840; row++) {
            for (int column = 28 * width / 840; column < 175 * width / 840; column++) {
                int pixel = offset + ((height - 1 - row) * width + column) * 4;
                if (bitmap[pixel] > 140 && bitmap[pixel + 1] > 140 && bitmap[pixel + 2] > 140) brightPixels++;
            }
        }
        if (brightPixels < 25) throw new InvalidOperationException("Settings heading disappeared after table edits");
        GetWindowRect(window, out Rectangle parent);
        using (var log = new StreamWriter(path + ".bounds.txt")) {
            for (int identifier=100; identifier<=320; identifier++) {
                IntPtr child=WindowsHarness.GetDlgItem(window, identifier);
                if (child == IntPtr.Zero || !WindowsHarness.IsWindowVisible(child)) continue;
                GetWindowRect(child, out Rectangle bounds);
                log.WriteLine(identifier + ": " + (bounds.Left-parent.Left) + "," + (bounds.Top-parent.Top) + ".." + (bounds.Right-parent.Left) + "," + (bounds.Bottom-parent.Top));
            }
        }
    }

    public static void Verify(IntPtr window, string directory, IntPtr process, bool physicalKeyboard) {
        SettingsHarness.Open(window);
        SettingsHarness.Click(window, 232);
        if (!WindowsHarness.IsWindowVisible(WindowsHarness.GetDlgItem(window, 300)) || WindowsHarness.IsWindowVisible(WindowsHarness.GetDlgItem(window, 303)))
            throw new InvalidOperationException("New table must contain exactly one blank row");
        SetField(window, 300, "https://example.com/docs?a=1&b=2");
        if (WindowsHarness.IsWindowVisible(WindowsHarness.GetDlgItem(window, 303))) throw new InvalidOperationException("Incomplete row appended another row");
        SetField(window, 301, "Core Quick Docs");
        SettingsHarness.WaitSaved(window);
        if (!WindowsHarness.IsWindowVisible(WindowsHarness.GetDlgItem(window, 303)) || WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 303)) != "")
            throw new InvalidOperationException("Completing a row did not append a blank row");
        SetField(window, 303, "C:\\My Files"); SetField(window, 304, "Project Files");
        SetField(window, 306, "example.com/reference"); SetField(window, 307, "Référence");
        SetField(window, 309, "STEAM://rungameid/1"); SetField(window, 310, "Quicklink 04");
        SettingsHarness.WaitSaved(window);
        CaptureTable(window, Path.Combine(directory, "quicklinks-table.bmp"));
        if (!WindowsHarness.IsWindowVisible(WindowsHarness.GetDlgItem(window, 320))) throw new InvalidOperationException("Growing table needs its scrollbar");
        if (physicalKeyboard) {
        IntPtr nameEditor = WindowsHarness.GetDlgItem(window, 310);
        DismissalHarness.Activate(window);
        SendMessageW(nameEditor, 0x0201, new IntPtr(1), new IntPtr(0x00080008));
        SendMessageW(nameEditor, 0x0202, IntPtr.Zero, new IntPtr(0x00080008));
        WindowsHarness.WaitFor(() => WindowsHarness.FocusedControl(window) == nameEditor, "Name field did not receive focus before the Tab test");
        var inputs = new[] {
            new Input { Type=1, Data=new InputUnion { Keyboard=new Keyboard { Key=0x10, Flags=2 } } },
            new Input { Type=1, Data=new InputUnion { Keyboard=new Keyboard { Key=0xA0, Flags=2 } } },
            new Input { Type=1, Data=new InputUnion { Keyboard=new Keyboard { Key=0xA1, Flags=2 } } },
            new Input { Type=1, Data=new InputUnion { Keyboard=new Keyboard { Key=9 } } },
            new Input { Type=1, Data=new InputUnion { Keyboard=new Keyboard { Key=9, Flags=2 } } }
        };
        if (SendInput((uint)inputs.Length, inputs, Marshal.SizeOf<Input>()) != inputs.Length) throw new InvalidOperationException("Windows rejected the Tab test input");
        WindowsHarness.WaitFor(() => WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 309)) == "", "Tab did not scroll to the appended row");
        SettingsHarness.Flush(window);
        if (WindowsHarness.FocusedControl(window) != WindowsHarness.GetDlgItem(window, 309) || WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 309)) != "")
            throw new InvalidOperationException("Tab did not reveal and focus the new blank row; focus=" + WindowsHarness.FocusedControl(window) + ", expected=" + WindowsHarness.GetDlgItem(window, 309) + ", text=" + WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 309)));
        }
        uint initialGdi = WindowsHarness.GetGuiResources(process, 0);
        for (int index = 5; index <= 30; index++) {
            Scroll(window, 7);
            SetField(window, 309, "https://example.com/" + index);
            SetField(window, 310, "Quicklink " + index.ToString("00"));
        }
        SettingsHarness.WaitSaved(window);
        if (WindowsHarness.GetGuiResources(process, 0) > initialGdi + 2) throw new InvalidOperationException("Growing table leaked graphics handles");
        CaptureTable(window, Path.Combine(directory, "quicklinks-many.bmp"));
        Scroll(window, 7);
        SetField(window, 309, "javascript:alert(1)"); SetField(window, 310, "Rejected target");
        SettingsHarness.Click(window, 221);
        string validationMessage = WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 222));
        if (!SettingsHarness.IsOpen(window) || !validationMessage.Contains("That link type is not allowed."))
            throw new InvalidOperationException("Blocked app link did not show its validation error: " + validationMessage);
        SetField(window, 309, "https://example.com/31"); SetField(window, 310, "Project Files");
        if (!WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 222)).Contains("unique")) throw new InvalidOperationException("Duplicate name accepted");
        SetField(window, 310, "Quicklink 31"); SettingsHarness.WaitSaved(window);
        CaptureTable(window, Path.Combine(directory, "quicklinks-corrected.bmp"));
        Scroll(window, 6);
        SetField(window, 301, "Core Documentation"); SettingsHarness.WaitSaved(window);
        CaptureTable(window, Path.Combine(directory, "quicklinks-renamed.bmp"));
        SettingsHarness.Click(window, 305); // Delete the saved file-path row.
        SettingsHarness.WaitSaved(window);
        CaptureTable(window, Path.Combine(directory, "quicklinks-edited.bmp"));
        SettingsHarness.Done(window);
        Query(window, "Core Documentation", "Core Documentation");
        Query(window, "> Quicklink 31", "Quicklink 31");
        Query(window, "@quicklink Référence", "Référence");
        WindowsHarness.Enter(window);
        WindowsHarness.WaitFor(() => WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 102)).Contains("no side effect"), "Quicklink did not reach dry-run activation");
        Query(window, "> Quicklink 04", "Quicklink 04");
        WindowsHarness.Enter(window);
        WindowsHarness.WaitFor(() => WindowsHarness.Text(WindowsHarness.GetDlgItem(window, 102)).Contains("no side effect"), "App link did not reach dry-run activation");
        WindowsHarness.Query(window, "> Project Files");
        WindowsHarness.WaitFor(() => WindowsHarness.Count(window) == 0, "Deleted quicklink remained searchable");
        SettingsHarness.Open(window); SettingsHarness.Click(window, 232); Scroll(window, 7);
        SetField(window, 309, "https://example.com/incomplete");
        WindowsHarness.PostMessageW(WindowsHarness.GetDlgItem(window, 309), 0x0100, new IntPtr(27), IntPtr.Zero);
        WindowsHarness.WaitFor(() => !SettingsHarness.IsOpen(window), "Escape did not dismiss the partial row");
    }
}
