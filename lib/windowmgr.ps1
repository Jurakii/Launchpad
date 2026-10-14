param(
  [string]$ExcludePid = '0'
)

# Long-lived window-manager helper for the organizer: moves other apps'
# windows into layout zones, and reports when the user starts and stops
# dragging one. Unlike the other lib/*.ps1 helpers it isn't spawned per
# call - compiling the C# below takes a few hundred ms - so windowmgr.js
# starts it once and talks to it over stdin/stdout: one JSON request per
# line in, one JSON reply per line out, matched by "id". When Launchpad
# closes stdin (or dies, which closes the pipe), ReadLine returns null and
# the helper exits on its own.
#
# Requests (all rects are physical pixels, [left, top, right, bottom]):
#   monitor  {hwnd}             -> {bounds, work} of the monitor the window is on
#   place    {hwnd, x, y, w, h} -> moves the window so its visible frame fills the rect
#   focus    {hwnd}             -> restores it if minimized and brings it to the front
# Failures reply {ok:false, code}: "gone" (window closed), "denied" (an
# elevated window Windows won't let us move), "failed", or "badRequest".
#
# Events (unprompted lines with "ev" instead of "id"), for windows of any
# process except ExcludePid (Launchpad itself):
#   dragStart {hwnd}            - the user is moving (not resizing) a window
#                                 worth snapping. Sent once per drag.
#   moveEnd   {hwnd, cancelled} - a window's move/size loop ended. cancelled =
#                                 Esc, seen as the mouse button still held.
Add-Type -ReferencedAssemblies 'System.Web.Extensions' -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;

public static class WindowManager {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)]
    public struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam; public IntPtr lParam; public uint time; public POINT pt; }
    [StructLayout(LayoutKind.Sequential)]
    public struct MONITORINFO { public uint cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags; }

    public delegate void WinEventProc(IntPtr hook, uint ev, IntPtr hwnd, int idObject, int idChild, uint thread, uint time);

    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr ctx);
    [DllImport("user32.dll")] static extern IntPtr SetWinEventHook(uint eventMin, uint eventMax, IntPtr hmod, WinEventProc proc, uint idProcess, uint idThread, uint flags);
    [DllImport("user32.dll")] static extern bool UnhookWinEvent(IntPtr hook);
    [DllImport("user32.dll")] static extern int GetMessage(out MSG msg, IntPtr hwnd, uint min, uint max);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG msg);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessage(ref MSG msg);
    [DllImport("user32.dll")] static extern bool IsWindow(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool IsZoomed(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr hwnd, int cmd);
    [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll", SetLastError = true)] static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out RECT rc);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr hwnd, uint cmd);
    [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr hwnd, int index);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out RECT rc, int size);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out int value, int size);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr hwnd, uint flags);
    [DllImport("user32.dll")] static extern bool GetMonitorInfo(IntPtr hMon, ref MONITORINFO info);
    [DllImport("user32.dll")] static extern short GetAsyncKeyState(int vKey);
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int index);
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("advapi32.dll")] static extern bool OpenProcessToken(IntPtr proc, uint access, out IntPtr token);
    [DllImport("advapi32.dll")] static extern bool GetTokenInformation(IntPtr token, int cls, out int info, int len, out int retLen);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);

    static readonly IntPtr PER_MONITOR_AWARE_V2 = new IntPtr(-4);
    const uint EVENT_SYSTEM_MOVESIZESTART = 0x000A, EVENT_SYSTEM_MOVESIZEEND = 0x000B;
    const uint EVENT_OBJECT_LOCATIONCHANGE = 0x800B;
    const uint WINEVENT_SKIPOWNPROCESS = 0x0002;
    const int DWMWA_EXTENDED_FRAME_BOUNDS = 9, DWMWA_CLOAKED = 14;
    const uint SWP_NOZORDER = 0x0004, SWP_NOACTIVATE = 0x0010, SWP_NOOWNERZORDER = 0x0200;
    const int SW_RESTORE = 9;
    const int ERROR_ACCESS_DENIED = 5;
    const int GWL_STYLE = -16, GWL_EXSTYLE = -20;
    const int WS_THICKFRAME = 0x00040000, WS_EX_TOOLWINDOW = 0x00000080;

    static readonly JavaScriptSerializer json = new JavaScriptSerializer();
    static readonly object outLock = new object();
    static WinEventProc hookProc; // a static field keeps the delegate alive; a collected one crashes the hook
    static uint excludePid;
    // The window whose move/size loop is running, while it's still unclear
    // whether the user is moving or resizing it, and its frame when last
    // looked at (hook thread only).
    static IntPtr pendingHwnd = IntPtr.Zero;
    static RECT pendingFrame;
    static IntPtr locationHook = IntPtr.Zero;
    static readonly uint selfPid = (uint)Process.GetCurrentProcess().Id;

    // Replies (command thread) and events (hook thread) share stdout.
    static void Emit(Dictionary<string, object> obj) {
        string line = json.Serialize(obj);
        lock (outLock) {
            Console.Out.WriteLine(line);
            Console.Out.Flush();
        }
    }

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

    // UIPI stops a normal process from moving an elevated window.
    static bool BlockedByElevation(uint pid) {
        return Elevation(pid) == 1 && Elevation(selfPid) != 1;
    }

    static Dictionary<string, object> Fail(string code) {
        var o = new Dictionary<string, object>();
        o["ok"] = false;
        o["code"] = code;
        return o;
    }

    // ---------- drag events (hook thread) ----------

    static void HookThread() {
        SetThreadDpiAwarenessContext(PER_MONITOR_AWARE_V2);
        hookProc = OnEvent;
        SetWinEventHook(EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND, IntPtr.Zero, hookProc, 0, 0, WINEVENT_SKIPOWNPROCESS);
        MSG msg;
        while (GetMessage(out msg, IntPtr.Zero, 0, 0) > 0) {
            TranslateMessage(ref msg);
            DispatchMessage(ref msg);
        }
    }

    // A move/size loop starts for title-bar drags, border resizes and the
    // keyboard Move/Size commands alike, so the start event alone doesn't say
    // which. Asking the window what's under the cursor isn't reliable: these
    // events arrive a moment late, and a quick hand can already have carried
    // the cursor from a border onto the title bar or vice versa. What the
    // window does is reliable: while it's being moved its size stays the
    // same and its position changes; while it's being resized, every change
    // is a size change. So the window is watched until it moves without
    // resizing. Ignoring size changes also covers a snapped window that
    // restores its normal size as the drag starts. A maximized window can't
    // be resized, so dragging one is a move straight away.
    static void OnEvent(IntPtr hook, uint ev, IntPtr hwnd, int idObject, int idChild, uint thread, uint time) {
        try {
            if (idObject != 0 || idChild != 0 || hwnd == IntPtr.Zero) return; // the window itself, not a part of it

            if (ev == EVENT_OBJECT_LOCATIONCHANGE) {
                if (hwnd != pendingHwnd) return; // other windows of the same app
                RECT now = Frame(hwnd);
                bool sameSize = now.Right - now.Left == pendingFrame.Right - pendingFrame.Left
                    && now.Bottom - now.Top == pendingFrame.Bottom - pendingFrame.Top;
                bool samePlace = now.Left == pendingFrame.Left && now.Top == pendingFrame.Top;
                // Events queue up and are handled after the fact, so several
                // can read the same current frame: one that matches the last
                // reading says nothing either way.
                if (sameSize && samePlace) return;
                if (sameSize) {
                    StopLocationWatch();
                    EmitDragStart(hwnd);
                } else {
                    pendingFrame = now;
                }
                return;
            }

            uint pid;
            GetWindowThreadProcessId(hwnd, out pid);
            if (pid == excludePid) return;

            if (ev == EVENT_SYSTEM_MOVESIZESTART) {
                StopLocationWatch();
                if (!IsSnappable(hwnd, pid)) return;
                if (IsZoomed(hwnd)) {
                    EmitDragStart(hwnd);
                } else {
                    pendingHwnd = hwnd;
                    pendingFrame = Frame(hwnd);
                    // Scoped to that one process, and only until the loop ends.
                    locationHook = SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE, IntPtr.Zero, hookProc, pid, 0, 0);
                }
                return;
            }

            // EVENT_SYSTEM_MOVESIZEEND
            StopLocationWatch();
            var o = new Dictionary<string, object>();
            o["ev"] = "moveEnd";
            o["hwnd"] = hwnd.ToInt64();
            // Esc cancels a move while the button is still held; a normal
            // drop ends the loop because the button was released.
            int button = GetSystemMetrics(23) != 0 ? 0x02 : 0x01; // SM_SWAPBUTTON -> VK_RBUTTON : VK_LBUTTON
            o["cancelled"] = (GetAsyncKeyState(button) & 0x8000) != 0;
            Emit(o);
        } catch (Exception e) {
            var err = new Dictionary<string, object>();
            err["ev"] = "hookError";
            err["error"] = e.Message;
            Emit(err);
        }
    }

    static void EmitDragStart(IntPtr hwnd) {
        var o = new Dictionary<string, object>();
        o["ev"] = "dragStart";
        o["hwnd"] = hwnd.ToInt64();
        Emit(o);
    }

    static void StopLocationWatch() {
        if (locationHook != IntPtr.Zero) UnhookWinEvent(locationHook);
        locationHook = IntPtr.Zero;
        pendingHwnd = IntPtr.Zero;
    }

    // Worth offering zones for: the windows the taskbar would show (same
    // rules as lib/windows.ps1), resizable ones only - a fixed-size dialog
    // stretched into a zone just looks broken - and not elevated ones we
    // couldn't move anyway.
    static bool IsSnappable(IntPtr h, uint pid) {
        if (!IsWindowVisible(h)) return false;
        if (GetWindow(h, 4) != IntPtr.Zero) return false; // GW_OWNER
        if ((GetWindowLong(h, GWL_EXSTYLE) & WS_EX_TOOLWINDOW) != 0) return false;
        if ((GetWindowLong(h, GWL_STYLE) & WS_THICKFRAME) == 0) return false;
        int cloaked;
        DwmGetWindowAttribute(h, DWMWA_CLOAKED, out cloaked, sizeof(int));
        if (cloaked != 0) return false;
        return !BlockedByElevation(pid);
    }

    // ---------- commands (main thread) ----------

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
        // Saying so up front gives a clear message instead of a window that
        // just doesn't move.
        uint pid;
        GetWindowThreadProcessId(h, out pid);
        if (BlockedByElevation(pid)) return Fail("denied");

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

    public static void Run(uint exclude) {
        excludePid = exclude;
        // Window titles will pass through here later; the console's default
        // OEM code page would mangle anything non-ASCII.
        Console.OutputEncoding = new UTF8Encoding(false);
        // Physical pixels everywhere, regardless of any awareness powershell.exe
        // set for the process: the per-thread setting wins for this thread.
        SetThreadDpiAwarenessContext(PER_MONITOR_AWARE_V2);

        var hookThread = new Thread(HookThread);
        hookThread.IsBackground = true; // don't keep the process alive once stdin closes
        hookThread.Start();

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
            Emit(res);
        }
    }
}
'@

[WindowManager]::Run([uint32]$ExcludePid)
