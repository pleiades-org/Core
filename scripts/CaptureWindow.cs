using System;
using System.IO;
using System.Runtime.InteropServices;

public static class CaptureWindow
{
    [StructLayout(LayoutKind.Sequential)] private struct Rectangle { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] private struct BitmapInfo {
        public uint Size; public int Width, Height; public ushort Planes, Bits;
        public uint Compression, ImageSize; public int XResolution, YResolution;
        public uint Colors, ImportantColors;
    }
    [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr window, out Rectangle rectangle);
    [DllImport("user32.dll")] private static extern IntPtr GetDC(IntPtr window);
    [DllImport("user32.dll")] private static extern int ReleaseDC(IntPtr window, IntPtr context);
    [DllImport("user32.dll")] private static extern bool PrintWindow(IntPtr window, IntPtr context, uint flags);
    [DllImport("dwmapi.dll")] private static extern int DwmFlush();
    [DllImport("gdi32.dll")] private static extern IntPtr CreateCompatibleDC(IntPtr context);
    [DllImport("gdi32.dll")] private static extern IntPtr CreateCompatibleBitmap(IntPtr context, int width, int height);
    [DllImport("gdi32.dll")] private static extern IntPtr SelectObject(IntPtr context, IntPtr bitmap);
    [DllImport("gdi32.dll")] private static extern bool DeleteObject(IntPtr bitmap);
    [DllImport("gdi32.dll")] private static extern bool DeleteDC(IntPtr context);
    [DllImport("gdi32.dll")] private static extern int GetDIBits(IntPtr context, IntPtr bitmap, uint start, uint lines, byte[] pixels, ref BitmapInfo info, uint usage);
    [DllImport("gdi32.dll")] private static extern bool BitBlt(IntPtr target, int x, int y, int width, int height, IntPtr source, int sourceX, int sourceY, uint operation);

    public static void Save(IntPtr window, string path)
    {
        Save(window, path, false);
    }

    public static void SaveVisible(IntPtr window, string path)
    {
        Save(window, path, true);
    }

    private static void Save(IntPtr window, string path, bool visibleSurface)
    {
        if (visibleSurface && DwmFlush() < 0) throw new IOException("Could not wait for the rendered window frame");
        if (!GetWindowRect(window, out Rectangle rectangle)) throw new IOException("Could not measure Core window");
        int width = rectangle.Right - rectangle.Left, height = rectangle.Bottom - rectangle.Top;
        IntPtr sourceWindow = visibleSurface ? IntPtr.Zero : window;
        IntPtr screen = GetDC(sourceWindow), context = CreateCompatibleDC(screen);
        IntPtr bitmap = CreateCompatibleBitmap(screen, width, height);
        IntPtr previous = SelectObject(context, bitmap);
        try {
            bool captured = visibleSurface
                ? BitBlt(context, 0, 0, width, height, screen, rectangle.Left, rectangle.Top, 0x00CC0020)
                : PrintWindow(window, context, 2);
            if (!captured) throw new IOException("Could not capture Core window");
            SelectObject(context, previous);
            var pixels = new byte[width * height * 4];
            var info = new BitmapInfo { Size=40, Width=width, Height=height, Planes=1, Bits=32, ImageSize=(uint)pixels.Length };
            if (GetDIBits(context, bitmap, 0, (uint)height, pixels, ref info, 0) != height) throw new IOException("Could not read captured pixels");
            using (var output = new BinaryWriter(File.Create(path))) {
                output.Write((ushort)0x4D42); output.Write(54 + pixels.Length); output.Write(0); output.Write(54);
                output.Write(40); output.Write(width); output.Write(height); output.Write((ushort)1); output.Write((ushort)32);
                output.Write(0); output.Write(pixels.Length); output.Write(0); output.Write(0); output.Write(0); output.Write(0);
                output.Write(pixels);
            }
        } finally {
            SelectObject(context, previous); DeleteObject(bitmap); DeleteDC(context); ReleaseDC(sourceWindow, screen);
        }
    }
}
