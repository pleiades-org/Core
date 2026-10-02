using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

public static class InstallerHarness
{
    public const int PrimaryId = 1210;
    public const int CancelId = 1211;
    public const int DesktopId = 1212;
    public const int StatusId = 1207;
    private const uint CommandMessage = 0x0111;
    private const uint CloseMessage = 0x0010;
    private const uint GetTextMessage = 0x000D;
    private delegate bool WindowCallback(IntPtr window, IntPtr parameter);

    [DllImport("user32.dll")] private static extern bool EnumWindows(WindowCallback callback, IntPtr parameter);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll", SetLastError=true)] private static extern bool RedrawWindow(IntPtr window, IntPtr updateRectangle, IntPtr updateRegion, uint flags);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetClassNameW(IntPtr window, StringBuilder className, int maximum);
    [DllImport("user32.dll")] private static extern IntPtr GetDlgItem(IntPtr window, int identifier);
    [DllImport("user32.dll", CharSet=CharSet.Unicode, EntryPoint="SendMessageW")] private static extern IntPtr ReadMessage(IntPtr window, uint message, IntPtr word, StringBuilder text);
    [DllImport("user32.dll")] private static extern bool PostMessageW(IntPtr window, uint message, IntPtr word, IntPtr data);
    [DllImport("user32.dll")] private static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr word, IntPtr data);
    [DllImport("kernel32.dll", SetLastError=true)] private static extern bool GetExitCodeProcess(IntPtr process, out uint exitCode);

    public static IntPtr FindWindow(int processId)
    {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, parameter) => {
            GetWindowThreadProcessId(window, out uint owner);
            var className = new StringBuilder(128);
            GetClassNameW(window, className, className.Capacity);
            if ((processId == 0 || owner == processId) && className.ToString() == "Pleiades.Core.Setup") {
                found = window;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static IntPtr FindForDirectory(string directory)
    {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, parameter) => {
            var className = new StringBuilder(128);
            GetClassNameW(window, className, className.Capacity);
            if (className.ToString() == "Pleiades.Core.Setup" && IsWindowVisible(window) && Text(window, 1204).Equals(directory, StringComparison.OrdinalIgnoreCase)) {
                found = window;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static int ProcessId(IntPtr window)
    {
        GetWindowThreadProcessId(window, out uint processId);
        return checked((int)processId);
    }

    public static int ExitCode(IntPtr process)
    {
        if (!GetExitCodeProcess(process, out uint exitCode))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Could not read setup exit code");
        return checked((int)exitCode);
    }

    public static string Text(IntPtr window, int identifier)
    {
        var text = new StringBuilder(8192);
        ReadMessage(GetDlgItem(window, identifier), GetTextMessage, new IntPtr(text.Capacity), text);
        return text.ToString();
    }

    public static void Click(IntPtr window, int identifier)
    {
        if (!PostMessageW(window, CommandMessage, new IntPtr(identifier), IntPtr.Zero))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Could not click setup control");
    }

    public static void EnterOnControl(IntPtr window, int identifier)
    {
        IntPtr control = GetDlgItem(window, identifier);
        SendMessageW(window, 0x0028, control, new IntPtr(1));
        if (WindowsHarness.FocusedControl(window) != control)
            throw new InvalidOperationException("Setup did not focus the requested keyboard control");
        if (!PostMessageW(window, 0x0100, new IntPtr(13), IntPtr.Zero))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Could not send Enter to setup");
    }

    public static void VerifyAppearance(IntPtr window, string screenshot, uint backgroundRgb)
    {
        WindowsHarness.WaitFor(() => {
            const uint InvalidateAndPaintChildren = 0x0001 | 0x0080 | 0x0100;
            if (!RedrawWindow(window, IntPtr.Zero, IntPtr.Zero, InvalidateAndPaintChildren))
                throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Could not paint setup before capture");
            CaptureWindow.Save(window, screenshot);
            byte[] bitmap = File.ReadAllBytes(screenshot);
            int offset = BitConverter.ToInt32(bitmap, 10);
            int width = BitConverter.ToInt32(bitmap, 18), height = BitConverter.ToInt32(bitmap, 22);
            double scale = GetDpiForWindow(window) / 96.0;
            int horizontal = (int)(10 * scale), vertical = (int)(70 * scale);
            uint background = BitConverter.ToUInt32(bitmap, offset + ((height - vertical - 1) * width + horizontal) * 4) & 0xffffff;
            if (background != backgroundRgb) return false;
            int contrastingPixels = 0;
            for (int row = (int)(28 * scale); row < (int)(65 * scale); row++) {
                for (int column = (int)(24 * scale); column < (int)(130 * scale); column++) {
                    uint pixel = BitConverter.ToUInt32(bitmap, offset + ((height - row - 1) * width + column) * 4) & 0xffffff;
                    if (pixel != backgroundRgb) contrastingPixels++;
                }
            }
            return contrastingPixels > 200
                && HasButtonText(bitmap, offset, width, height, scale, 340, 394, 96, 14)
                && HasButtonText(bitmap, offset, width, height, scale, 492, 394, 96, 14)
                && HasButtonText(bitmap, offset, width, height, scale, 40, 252, 180, 14);
        }, "Setup did not render its theme and title text");
    }

    private static bool HasButtonText(byte[] bitmap, int offset, int width, int height, double scale, int left, int top, int logicalWidth, int logicalHeight)
    {
        uint firstPixel = BitConverter.ToUInt32(bitmap, offset + ((height - (int)(top * scale) - 1) * width + (int)(left * scale)) * 4) & 0xffffff;
        int contrastingPixels = 0;
        for (int row = (int)(top * scale); row < (int)((top + logicalHeight) * scale); row++) {
            for (int column = (int)(left * scale); column < (int)((left + logicalWidth) * scale); column++) {
                uint pixel = BitConverter.ToUInt32(bitmap, offset + ((height - row - 1) * width + column) * 4) & 0xffffff;
                if (pixel != firstPixel) contrastingPixels++;
            }
        }
        return contrastingPixels > 40;
    }

    public static void Close(IntPtr window)
    {
        if (!PostMessageW(window, CloseMessage, IntPtr.Zero, IntPtr.Zero))
            throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error(), "Could not close setup window");
    }
}
