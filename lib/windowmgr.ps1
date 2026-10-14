# Long-lived window-manager helper for the organizer: moves other apps'
# windows into layout zones. Unlike the other lib/*.ps1 helpers it isn't
# spawned per call - compiling the C# below takes a few hundred ms - so
# windowmgr.js starts it once, on first use, and talks to it over
# stdin/stdout: one JSON request per line in, one JSON reply per line out,
# matched by "id". When Launchpad closes stdin (or dies, which closes the
# pipe), ReadLine returns null and the helper exits on its own.
#
# Requests (all rects are physical pixels, [left, top, right, bottom]):
#   monitor  {hwnd}             -> {bounds, work} of the monitor the window is on
#   place    {hwnd, x, y, w, h} -> moves the window so its visible frame fills the rect
#   focus    {hwnd}             -> restores it if minimized and brings it to the front
# Failures reply {ok:false, code}: "gone" (window closed), "denied" (an
# elevated window Windows won't let us move), "failed", or "badRequest".
Add-Type -ReferencedAssemblies 'System.Web.Extensions' -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Web.Script.Serialization;

public static class WindowManager {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct MONITORINFO { public uint cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags; }

    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr ctx);
    [DllImport("user32.dll")] static extern bool IsWindow(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool IsZoomed(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr hwnd, int cmd);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll", SetLastError = true)] static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out RECT rc);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out RECT rc, int size);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr hwnd, uint flags);
    [DllImport("user32.dll")] static extern bool GetMonitorInfo(IntPtr hMon, ref MONITORINFO info);
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("advapi32.dll")] static extern bool OpenProcessToken(IntPtr proc, uint access, out IntPtr token);
    [DllImport("advapi32.dll")] static extern bool GetTokenInformation(IntPtr token, int cls, out int info, int len, out int retLen);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);

    const int DWMWA_EXTENDED_FRAME_BOUNDS = 9;
    const uint SWP_NOZORDER = 0x0004, SWP_NOACTIVATE = 0x0010, SWP_NOOWNERZORDER = 0x0200;
    const int SW_RESTORE = 9;
    const int ERROR_ACCESS_DENIED = 5;

    static readonly JavaScriptSerializer json = new JavaScriptSerializer();

    static int[] R(RECT r) { return new int[] { r.Left, r.Top, r.Right, r.Bottom }; }

    // The visible frame. GetWindowRect also counts the ~7px invisible resize
    // border Windows 10 draws on the left, right and bottom of most windows;
    // the DWM frame bounds don't, and that's what should line up with a zone.
    static RECT Frame(IntPtr h) {
        RECT r;
        if (DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, out r, Marshal.SizeOf(typeof(RECT))) != 0) GetWindowRect(h, out r);
        return r;
    }

    // 1 = elevated, 0 = not, -1 = couldn't tell.
    static int Elevation(uint pid) {
        IntPtr p = OpenProcess(0x1000, false, pid); // PROCESS_QUERY_LIMITED_INFORMATION
        if (p == IntPtr.Zero) return -1;
        try {
            IntPtr t;
            if (!OpenProcessToken(p, 0x0008, out t)) return -1; // TOKEN_QUERY
            try {
                int elevated, len;
                if (!GetTokenInformation(t, 20, out elevated, 4, out len)) return -1; // TokenElevation
                return elevated != 0 ? 1 : 0;
            } finally { CloseHandle(t); }
        } finally { CloseHandle(p); }
    }

    static Dictionary<string, object> Fail(string code) {
        var o = new Dictionary<string, object>();
        o["ok"] = false;
        o["code"] = code;
        return o;
    }

    static Dictionary<string, object> Monitor(IntPtr h) {
        // For a minimized window this is the monitor it will restore onto.
        IntPtr mon = MonitorFromWindow(h, 2); // MONITOR_DEFAULTTONEAREST
        var mi = new MONITORINFO();
        mi.cbSize = (uint)Marshal.SizeOf(typeof(MONITORINFO));
        if (!GetMonitorInfo(mon, ref mi)) return Fail("failed");
        var o = new Dictionary<string, object>();
        o["bounds"] = R(mi.rcMonitor);
        o["work"] = R(mi.rcWork);
        return o;
    }

    static Dictionary<string, object> Place(IntPtr h, int x, int y, int w, int hgt) {
        // UIPI stops a normal process from moving an elevated window. Saying
        // so up front gives a clear message instead of a window that just
        // doesn't move.
        uint pid;
        GetWindowThreadProcessId(h, out pid);
        if (Elevation(pid) == 1 && Elevation((uint)Process.GetCurrentProcess().Id) != 1) return Fail("denied");

        // Aero Snap may already have maximized it (it does so before the
        // drag-end event arrives), and a maximized window ignores SetWindowPos.
        if (IsZoomed(h) || IsIconic(h)) ShowWindow(h, SW_RESTORE);

        // Two passes at most: moving to a monitor with a different DPI makes
        // the app rescale itself and changes its invisible border, so the
        // first pass can land slightly off.
        int off = int.MaxValue;
        for (int pass = 1; pass <= 2 && off > 1; pass++) {
            RECT wr;
            GetWindowRect(h, out wr);
            RECT fr = Frame(h);
            int dl = fr.Left - wr.Left, dt = fr.Top - wr.Top, dr = wr.Right - fr.Right, db = wr.Bottom - fr.Bottom;
            if (!SetWindowPos(h, IntPtr.Zero, x - dl, y - dt, w + dl + dr, hgt + dt + db, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER)) {
                return Fail(Marshal.GetLastWin32Error() == ERROR_ACCESS_DENIED ? "denied" : "failed");
            }
            RECT after = Frame(h);
            off = Math.Max(Math.Max(Math.Abs(after.Left - x), Math.Abs(after.Top - y)),
                           Math.Max(Math.Abs(after.Right - (x + w)), Math.Abs(after.Bottom - (y + hgt))));
        }
        var o = new Dictionary<string, object>();
        o["frame"] = R(Frame(h));
        o["exact"] = off <= 1; // false when the app enforces a minimum size bigger than the zone
        return o;
    }

    static Dictionary<string, object> Focus(IntPtr h) {
        if (IsIconic(h)) ShowWindow(h, SW_RESTORE);
        var o = new Dictionary<string, object>();
        o["ok"] = SetForegroundWindow(h);
        return o;
    }

    static int I(Dictionary<string, object> r, string k) { return Convert.ToInt32(r[k]); }

    static Dictionary<string, object> Handle(Dictionary<string, object> r) {
        var h = new IntPtr(Convert.ToInt64(r["hwnd"]));
        if (!IsWindow(h)) return Fail("gone");
        switch ((string)r["cmd"]) {
            case "monitor": return Monitor(h);
            case "place": return Place(h, I(r, "x"), I(r, "y"), I(r, "w"), I(r, "h"));
            case "focus": return Focus(h);
            default: return Fail("badRequest");
        }
    }

    public static void Run() {
        // Window titles will pass through here later; the console's default
        // OEM code page would mangle anything non-ASCII.
        Console.OutputEncoding = new UTF8Encoding(false);
        // Physical pixels everywhere, regardless of any awareness powershell.exe
        // set for the process: the per-thread setting wins for this thread.
        SetThreadDpiAwarenessContext(new IntPtr(-4)); // PER_MONITOR_AWARE_V2

        string line;
        while ((line = Console.In.ReadLine()) != null) {
            if (line.Trim().Length == 0) continue;
            object id = null;
            Dictionary<string, object> res;
            try {
                var req = json.Deserialize<Dictionary<string, object>>(line);
                id = req.ContainsKey("id") ? req["id"] : null;
                res = Handle(req);
            } catch (Exception e) {
                res = Fail("badRequest");
                res["error"] = e.Message;
            }
            res["id"] = id;
            if (!res.ContainsKey("ok")) res["ok"] = true;
            Console.Out.WriteLine(json.Serialize(res));
            Console.Out.Flush();
        }
    }
}
'@

[WindowManager]::Run()
