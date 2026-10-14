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
#   monitor      {hwnd}             -> {bounds, work} of the monitor the window is on,
#                                      plus the window's visible frame and whether
#                                      it's minimized
#   place        {hwnd, x, y, w, h} -> moves the window so its visible frame fills the rect
#   focus        {hwnd}             -> restores it if minimized and brings it to the front
#   appbarSet    {hwnd, msg, x, y, w, h, renew}
#                                   -> docks the window along the top of the rect as an
#                                      AppBar (registering it first), replies {rect}
#   appbarRemove {hwnd}             -> gives the AppBar's space back
#   fullscreen   {own}              -> {monitors}: the bounds of each monitor a
#                                      fullscreen window is covering right now.
#                                      own lists windows (Launchpad's main window)
#                                      that never count, even when they cover one
# Failures reply {ok:false, code}: "gone" (window closed), "denied" (an
# elevated window Windows won't let us move), "failed", or "badRequest".
#
# Events (unprompted lines with "ev" instead of "id"):
#   ready      {taskbarCreated}  - started. taskbarCreated is the message Explorer
#                                  broadcasts when it restarts, forgetting AppBars.
#   foreground {hwnd, kind, visible, title, exe, snappable}
#                                - the foreground window changed, or its title did.
#                                  kind: "app", "own" (Launchpad, ExcludePid),
#                                  "shell" (desktop, taskbar, Start, search) or
#                                  "none". visible is false for the hidden windows
#                                  some background apps bring to the front.
#                                  Also sent once at startup.
#   dragStart  {hwnd}            - the user is moving (not resizing) a window
#                                  worth snapping. Sent once per drag. Never for
#                                  Launchpad's own windows.
#   moveEnd    {hwnd, cancelled} - a window's move/size loop ended. cancelled =
#                                  Esc, seen as the mouse button still held.
#
# AppBars registered here are removed again when stdin closes. That only
# covers a clean exit: if Launchpad crashes, Windows kills this helper with
# it, so appbar-guard.ps1 removes whatever is left (see there).
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
    [StructLayout(LayoutKind.Sequential)]
    public struct APPBARDATA { public uint cbSize; public IntPtr hWnd; public uint uCallbackMessage; public uint uEdge; public RECT rc; public IntPtr lParam; }

    public delegate void WinEventProc(IntPtr hook, uint ev, IntPtr hwnd, int idObject, int idChild, uint thread, uint time);
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr lParam);
    public delegate bool MonitorEnumProc(IntPtr hMon, IntPtr hdc, IntPtr rect, IntPtr lParam);

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
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool QueryFullProcessImageName(IntPtr proc, uint flags, StringBuilder name, ref int size);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int max);
    [DllImport("user32.dll")] static extern int GetWindowTextLength(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder name, int max);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowsProc proc, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr clip, MonitorEnumProc proc, IntPtr lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern uint RegisterWindowMessage(string name);
    [DllImport("shell32.dll")] static extern UIntPtr SHAppBarMessage(uint msg, ref APPBARDATA data);

    static readonly IntPtr PER_MONITOR_AWARE_V2 = new IntPtr(-4);
    const uint EVENT_SYSTEM_FOREGROUND = 0x0003;
    const uint EVENT_SYSTEM_MOVESIZESTART = 0x000A, EVENT_SYSTEM_MOVESIZEEND = 0x000B;
    const uint EVENT_OBJECT_LOCATIONCHANGE = 0x800B, EVENT_OBJECT_NAMECHANGE = 0x800C;
    const uint WINEVENT_SKIPOWNPROCESS = 0x0002;
    const int DWMWA_EXTENDED_FRAME_BOUNDS = 9, DWMWA_CLOAKED = 14;
    const uint SWP_NOZORDER = 0x0004, SWP_NOACTIVATE = 0x0010, SWP_NOOWNERZORDER = 0x0200;
    const int SW_RESTORE = 9;
    const int ERROR_ACCESS_DENIED = 5;
    const int GWL_STYLE = -16, GWL_EXSTYLE = -20;
    const int WS_THICKFRAME = 0x00040000, WS_EX_TOOLWINDOW = 0x00000080;
    const int WS_EX_TOPMOST = 0x00000008, WS_EX_TRANSPARENT = 0x00000020, WS_EX_NOACTIVATE = 0x08000000;
    const uint ABM_NEW = 0, ABM_REMOVE = 1, ABM_QUERYPOS = 2, ABM_SETPOS = 3, ABE_TOP = 1;

    // Windows that belong to the shell rather than to an app: the desktop,
    // the taskbars, and the Start, search and Action Center hosts.
    static readonly string[] shellClasses = { "Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd" };
    static readonly string[] shellProcesses = { "StartMenuExperienceHost", "SearchHost", "SearchApp", "ShellExperienceHost" };

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
    // The foreground window, and a hook for title changes scoped to its
    // process (hook thread only).
    static IntPtr foregroundHwnd = IntPtr.Zero;
    static uint foregroundPid;
    static IntPtr nameHook = IntPtr.Zero;
    static readonly uint selfPid = (uint)Process.GetCurrentProcess().Id;
    // AppBars this helper registered, and the rect each was last given
    // (command thread only).
    static readonly Dictionary<long, RECT> appbars = new Dictionary<long, RECT>();
    static string guardScript;
    static Process guard;

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

    static string ClassOf(IntPtr h) {
        var sb = new StringBuilder(256);
        GetClassName(h, sb, sb.Capacity);
        return sb.ToString();
    }

    // Can't hang on a busy app: for another process's window this reads the
    // stored text instead of sending the window WM_GETTEXT.
    static string TitleOf(IntPtr h) {
        int len = GetWindowTextLength(h);
        if (len <= 0) return "";
        var sb = new StringBuilder(len + 1);
        GetWindowText(h, sb, sb.Capacity);
        return sb.ToString();
    }

    static string ExePath(uint pid) {
        IntPtr p = OpenProcess(0x1000, false, pid); // PROCESS_QUERY_LIMITED_INFORMATION, allowed even for elevated apps
        if (p == IntPtr.Zero) return null;
        try {
            var sb = new StringBuilder(1024);
            int size = sb.Capacity;
            return QueryFullProcessImageName(p, 0, sb, ref size) ? sb.ToString() : null;
        } finally { CloseHandle(p); }
    }

    static bool IsShell(IntPtr h, string exe) {
        if (Array.IndexOf(shellClasses, ClassOf(h)) >= 0) return true;
        if (exe == null) return false;
        string name = System.IO.Path.GetFileNameWithoutExtension(exe);
        foreach (string s in shellProcesses) {
            if (string.Equals(s, name, StringComparison.OrdinalIgnoreCase)) return true;
        }
        return false;
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

    // ---------- foreground and drag events (hook thread) ----------

    static void HookThread() {
        SetThreadDpiAwarenessContext(PER_MONITOR_AWARE_V2);
        hookProc = OnEvent;
        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, IntPtr.Zero, hookProc, 0, 0, WINEVENT_SKIPOWNPROCESS);
        SetWinEventHook(EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND, IntPtr.Zero, hookProc, 0, 0, WINEVENT_SKIPOWNPROCESS);
        // Whatever is in front already, so Launchpad doesn't have to wait
        // for the user to switch windows to find out.
        OnEvent(IntPtr.Zero, EVENT_SYSTEM_FOREGROUND, GetForegroundWindow(), 0, 0, 0, 0);
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

            if (ev == EVENT_SYSTEM_FOREGROUND) {
                if (nameHook != IntPtr.Zero) UnhookWinEvent(nameHook);
                nameHook = IntPtr.Zero;
                foregroundHwnd = hwnd;
                GetWindowThreadProcessId(hwnd, out foregroundPid);
                // Switching tabs or documents changes the title but not the
                // window, so title changes are watched too, in that process only.
                if (foregroundPid != 0) {
                    nameHook = SetWinEventHook(EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE, IntPtr.Zero, hookProc, foregroundPid, 0, 0);
                }
                EmitForeground();
                return;
            }

            if (ev == EVENT_OBJECT_NAMECHANGE) {
                if (hwnd == foregroundHwnd) EmitForeground();
                return;
            }

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

    static void EmitForeground() {
        IntPtr h = foregroundHwnd;
        string kind, exe = null;
        if (h == IntPtr.Zero || !IsWindow(h)) kind = "none";
        else if (foregroundPid == excludePid) kind = "own";
        else {
            exe = ExePath(foregroundPid);
            kind = IsShell(h, exe) ? "shell" : "app";
        }
        var o = new Dictionary<string, object>();
        o["ev"] = "foreground";
        o["hwnd"] = h.ToInt64();
        o["kind"] = kind;
        o["visible"] = kind != "none" && IsWindowVisible(h);
        o["title"] = kind == "app" || kind == "own" ? TitleOf(h) : "";
        o["exe"] = exe;
        o["snappable"] = kind == "app" && IsSnappable(h, foregroundPid);
        Emit(o);
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
        o["frame"] = R(Frame(h));
        o["minimized"] = IsIconic(h);
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

    static APPBARDATA AppBarData(IntPtr h) {
        var d = new APPBARDATA();
        d.cbSize = (uint)Marshal.SizeOf(typeof(APPBARDATA));
        d.hWnd = h;
        return d;
    }

    // Keeps appbar-guard.ps1 up to date with the AppBars that are registered.
    // It's started with the first one; a guard that has died is replaced and
    // told about all of them again.
    static void TellGuard(string verb, IntPtr h) {
        try {
            if (guard == null || guard.HasExited) {
                if (verb != "add") return;
                var psi = new ProcessStartInfo("powershell.exe",
                    "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"" + guardScript + "\"");
                psi.UseShellExecute = false;
                psi.RedirectStandardInput = true;
                psi.CreateNoWindow = true;
                guard = Process.Start(psi);
                guard.StandardInput.AutoFlush = true;
                foreach (long key in appbars.Keys) guard.StandardInput.WriteLine("add " + key);
            }
            guard.StandardInput.WriteLine(verb + " " + h.ToInt64());
        } catch (Exception) {
            // Without a guard, only a crash would leave the space reserved.
        }
    }

    static bool SameRect(RECT a, RECT b) {
        return a.Left == b.Left && a.Top == b.Top && a.Right == b.Right && a.Bottom == b.Bottom;
    }

    // Registers Launchpad's panel window as an AppBar on its behalf, so the
    // strip it covers comes out of the work area and maximized windows stop
    // below it. Explorer sends its notifications (msg) to the window itself,
    // where Electron's hookWindowMessage picks them up.
    static Dictionary<string, object> AppBarSet(IntPtr h, uint msg, int x, int y, int w, int hgt, bool renew) {
        long key = h.ToInt64();
        if (renew) appbars.Remove(key); // Explorer restarted and forgot it
        if (!appbars.ContainsKey(key)) {
            var reg = AppBarData(h);
            reg.uCallbackMessage = msg;
            if (SHAppBarMessage(ABM_NEW, ref reg) == UIntPtr.Zero) {
                // Still registered by a helper that has since died: start
                // over so the callback message is set again.
                var old = AppBarData(h);
                SHAppBarMessage(ABM_REMOVE, ref old);
                if (SHAppBarMessage(ABM_NEW, ref reg) == UIntPtr.Zero) return Fail("failed");
            }
            appbars[key] = new RECT();
            TellGuard("add", h);
        }
        var d = AppBarData(h);
        d.uEdge = ABE_TOP;
        d.rc.Left = x;
        d.rc.Top = y;
        d.rc.Right = x + w;
        d.rc.Bottom = y + hgt;
        // Moves it below anything already docked there, like a Windows
        // taskbar at the top. Only the top edge is taken from the answer.
        SHAppBarMessage(ABM_QUERYPOS, ref d);
        d.rc.Bottom = d.rc.Top + hgt;
        // Setting the same rect again would still notify every other AppBar,
        // Launchpad's panels on other monitors included, and they'd answer by
        // setting theirs again.
        if (!SameRect(appbars[key], d.rc)) {
            SHAppBarMessage(ABM_SETPOS, ref d);
            appbars[key] = d.rc;
        }
        var o = new Dictionary<string, object>();
        o["rect"] = R(d.rc);
        return o;
    }

    // Works on a window that's already gone too: Explorer only needs the handle.
    static Dictionary<string, object> AppBarRemove(IntPtr h) {
        var d = AppBarData(h);
        SHAppBarMessage(ABM_REMOVE, ref d);
        appbars.Remove(h.ToInt64());
        TellGuard("remove", h);
        return new Dictionary<string, object>();
    }

    static void RemoveAllAppBars() {
        foreach (long key in new List<long>(appbars.Keys)) AppBarRemove(new IntPtr(key));
    }

    // A monitor counts as taken over by a fullscreen app when the first
    // window on it, going down the z-order, covers all of it. Passed over on
    // the way: the shell's own windows, windows that never take focus
    // (Launchpad's panels and overlays, notifications), click-through and
    // tool windows, and smaller always-on-top ones, so a sticky note can't
    // hide a fullscreen game from this. A maximized window doesn't count even
    // when it fills the monitor because nothing reserves any space. Nor does
    // an own window (Launchpad's main window, whose top bar is the panel):
    // it ends the walk like any other, so a fullscreen app behind it doesn't
    // count either.
    static Dictionary<string, object> Fullscreen(List<long> own) {
        var monitors = new List<RECT>();
        MonitorEnumProc onMonitor = (IntPtr hMon, IntPtr hdc, IntPtr rect, IntPtr lp) => {
            var mi = new MONITORINFO();
            mi.cbSize = (uint)Marshal.SizeOf(typeof(MONITORINFO));
            if (GetMonitorInfo(hMon, ref mi)) monitors.Add(mi.rcMonitor);
            return true;
        };
        EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, onMonitor, IntPtr.Zero);

        var windows = new List<IntPtr>(); // top of the z-order first
        EnumWindowsProc onWindow = (IntPtr h, IntPtr lp) => {
            if (!IsWindowVisible(h) || IsIconic(h)) return true;
            if ((GetWindowLong(h, GWL_EXSTYLE) & (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT)) != 0) return true;
            int cloaked;
            DwmGetWindowAttribute(h, DWMWA_CLOAKED, out cloaked, sizeof(int));
            if (cloaked != 0) return true;
            if (Array.IndexOf(shellClasses, ClassOf(h)) >= 0) return true;
            windows.Add(h);
            return true;
        };
        EnumWindows(onWindow, IntPtr.Zero);

        var covered = new List<int[]>();
        foreach (RECT m in monitors) {
            foreach (IntPtr h in windows) {
                RECT r;
                GetWindowRect(h, out r);
                if (r.Left >= m.Right || r.Right <= m.Left || r.Top >= m.Bottom || r.Bottom <= m.Top) continue; // not on this monitor
                bool covers = r.Left <= m.Left && r.Top <= m.Top && r.Right >= m.Right && r.Bottom >= m.Bottom;
                if (covers && !IsZoomed(h) && !own.Contains(h.ToInt64())) covered.Add(R(m));
                if (covers || (GetWindowLong(h, GWL_EXSTYLE) & WS_EX_TOPMOST) == 0) break;
            }
        }
        var o = new Dictionary<string, object>();
        o["monitors"] = covered;
        return o;
    }

    static int I(Dictionary<string, object> r, string k) { return Convert.ToInt32(r[k]); }

    static Dictionary<string, object> Handle(Dictionary<string, object> r) {
        string cmd = (string)r["cmd"];
        if (cmd == "fullscreen") {
            var own = new List<long>();
            object list;
            // JavaScriptSerializer hands arrays back as an ArrayList here.
            if (r.TryGetValue("own", out list) && list is System.Collections.IEnumerable && !(list is string)) {
                foreach (object o in (System.Collections.IEnumerable)list) own.Add(Convert.ToInt64(o));
            }
            return Fullscreen(own);
        }
        var h = new IntPtr(Convert.ToInt64(r["hwnd"]));
        if (cmd == "appbarRemove") return AppBarRemove(h);
        if (!IsWindow(h)) return Fail("gone");
        switch (cmd) {
            case "monitor": return Monitor(h);
            case "place": return Place(h, I(r, "x"), I(r, "y"), I(r, "w"), I(r, "h"));
            case "focus": return Focus(h);
            case "appbarSet":
                return AppBarSet(h, (uint)Convert.ToInt64(r["msg"]), I(r, "x"), I(r, "y"), I(r, "w"), I(r, "h"),
                    r.ContainsKey("renew") && Convert.ToBoolean(r["renew"]));
            default: return Fail("badRequest");
        }
    }

    public static void Run(uint exclude, string guardPath) {
        excludePid = exclude;
        guardScript = guardPath;
        // Window titles pass through here, and the console's default OEM code
        // page would mangle anything non-ASCII.
        Console.OutputEncoding = new UTF8Encoding(false);
        // Physical pixels everywhere, regardless of any awareness powershell.exe
        // set for the process: the per-thread setting wins for this thread.
        SetThreadDpiAwarenessContext(PER_MONITOR_AWARE_V2);

        var ready = new Dictionary<string, object>();
        ready["ev"] = "ready";
        ready["taskbarCreated"] = RegisterWindowMessage("TaskbarCreated");
        Emit(ready);

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
        // Launchpad quit or crashed (either closes the pipe): give back the
        // space its panels reserved.
        RemoveAllAppBars();
    }
}
'@

[WindowManager]::Run([uint32]$ExcludePid, (Join-Path $PSScriptRoot 'appbar-guard.ps1'))
