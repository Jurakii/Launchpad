param(
  [string]$ExcludePid = '0'
)

# Phase 0 spike of the organizer's long-lived window-manager helper. Not
# shipped (spikes/ isn't in build.files). It answers three questions:
#   1. Can a PowerShell-hosted C# class run SetWinEventHook with its own
#      message loop, and how quickly do move/size events reach Node?
#   2. Can this process register an Electron window as an AppBar on its behalf?
#   3. Does SetWindowPos land a window exactly in a zone on every monitor once
#      DPI and the invisible resize borders are accounted for?
#
# Protocol: one JSON object per line both ways. Requests carry an "id" that
# the reply echoes; unsolicited hook events carry "ev" instead. When the
# parent closes stdin, ReadLine returns null and the helper exits on its own,
# so there's no need to poll the parent PID the way foreground.ps1 does.
Add-Type -ReferencedAssemblies 'System.Web.Extensions' -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;

public static class WmSpike {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)]
    public struct MSG { public IntPtr hwnd; public uint message; public IntPtr wParam; public IntPtr lParam; public uint time; public POINT pt; }
    [StructLayout(LayoutKind.Sequential)]
    public struct APPBARDATA { public uint cbSize; public IntPtr hWnd; public uint uCallbackMessage; public uint uEdge; public RECT rc; public IntPtr lParam; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct MONITORINFOEX {
        public uint cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string szDevice;
    }

    public delegate void WinEventProc(IntPtr hook, uint ev, IntPtr hwnd, int idObject, int idChild, uint thread, uint time);
    public delegate bool MonitorEnumProc(IntPtr hMon, IntPtr hdc, ref RECT rc, IntPtr data);
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")] static extern IntPtr SetWinEventHook(uint eventMin, uint eventMax, IntPtr hmod, WinEventProc proc, uint idProcess, uint idThread, uint flags);
    [DllImport("user32.dll")] static extern int GetMessage(out MSG msg, IntPtr hWnd, uint min, uint max);
    [DllImport("user32.dll")] static extern bool TranslateMessage(ref MSG msg);
    [DllImport("user32.dll")] static extern IntPtr DispatchMessage(ref MSG msg);
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr ctx);
    [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool PhysicalToLogicalPointForPerMonitorDPI(IntPtr hwnd, ref POINT pt);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out RECT rc);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out RECT rc, int size);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out int value, int size);
    [DllImport("user32.dll")] static extern bool IsZoomed(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool IsIconic(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool IsWindow(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr hwnd, int cmd);
    [DllImport("user32.dll", SetLastError = true)] static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] static extern bool GetCursorPos(out POINT pt);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd, StringBuilder sb, int max);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder sb, int max);
    [DllImport("user32.dll")] static extern int GetWindowTextLength(IntPtr hwnd);
    [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr hwnd, uint cmd);
    [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr hwnd, int index);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowsProc proc, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr clip, MonitorEnumProc proc, IntPtr data);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool GetMonitorInfo(IntPtr hMon, ref MONITORINFOEX info);
    [DllImport("shcore.dll")] static extern int GetDpiForMonitor(IntPtr hMon, int type, out uint dpiX, out uint dpiY);
    [DllImport("shell32.dll")] static extern UIntPtr SHAppBarMessage(uint msg, ref APPBARDATA data);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern IntPtr SendMessageTimeout(IntPtr hwnd, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr proc, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError = true)] static extern bool GetTokenInformation(IntPtr token, int cls, out int info, int len, out int retLen);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);

    static readonly IntPtr PER_MONITOR_AWARE_V2 = new IntPtr(-4);
    const uint EVENT_SYSTEM_FOREGROUND = 0x0003;
    const uint EVENT_SYSTEM_MOVESIZESTART = 0x000A;
    const uint EVENT_SYSTEM_MOVESIZEEND = 0x000B;
    const uint WINEVENT_SKIPOWNPROCESS = 0x0002;
    const int DWMWA_EXTENDED_FRAME_BOUNDS = 9;
    const int DWMWA_CLOAKED = 14;
    const uint SWP_NOZORDER = 0x0004, SWP_NOACTIVATE = 0x0010, SWP_NOOWNERZORDER = 0x0200;

    static readonly object outLock = new object();
    static readonly JavaScriptSerializer json = new JavaScriptSerializer();
    static WinEventProc hookProc; // a static field keeps the delegate alive; a collected one crashes the hook
    static uint excludePid;
    static readonly Dictionary<IntPtr, RECT> dragStartFrames = new Dictionary<IntPtr, RECT>();

    static void Emit(Dictionary<string, object> obj) {
        string line = json.Serialize(obj);
        lock (outLock) { Console.Out.WriteLine(line); Console.Out.Flush(); }
    }

    static long EpochMs() {
        return (long)(DateTime.UtcNow - new DateTime(1970, 1, 1, 0, 0, 0, DateTimeKind.Utc)).TotalMilliseconds;
    }

    static int[] R(RECT r) { return new int[] { r.Left, r.Top, r.Right, r.Bottom }; }

    // The visible frame. GetWindowRect includes the ~7px invisible resize
    // border Windows 10 draws around most windows; DWM's extended frame
    // bounds don't, and that's the rect a zone should line up with.
    static RECT Frame(IntPtr h) {
        RECT r;
        if (DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, out r, Marshal.SizeOf(typeof(RECT))) != 0) GetWindowRect(h, out r);
        return r;
    }

    static string ExeOf(uint pid) {
        try { return Process.GetProcessById((int)pid).ProcessName; } catch { return null; }
    }

    static Dictionary<string, object> Describe(IntPtr h) {
        var o = new Dictionary<string, object>();
        o["hwnd"] = h.ToInt64();
        uint pid;
        GetWindowThreadProcessId(h, out pid);
        o["pid"] = (long)pid;
        o["exe"] = ExeOf(pid);
        var sb = new StringBuilder(512);
        GetWindowText(h, sb, sb.Capacity);
        o["title"] = sb.ToString();
        var cb = new StringBuilder(256);
        GetClassName(h, cb, cb.Capacity);
        o["class"] = cb.ToString();
        RECT wr;
        GetWindowRect(h, out wr);
        o["rect"] = R(wr);
        o["frame"] = R(Frame(h));
        o["zoomed"] = IsZoomed(h);
        o["dpi"] = (long)GetDpiForWindow(h);
        return o;
    }

    // Same rules as lib/windows.ps1: what the real taskbar would show.
    static bool IsTaskbarWindow(IntPtr h) {
        if (!IsWindowVisible(h)) return false;
        if (GetWindowTextLength(h) == 0) return false;
        if (GetWindow(h, 4) != IntPtr.Zero) return false; // GW_OWNER
        if ((GetWindowLong(h, -20) & 0x80) != 0) return false; // WS_EX_TOOLWINDOW
        int cloaked;
        DwmGetWindowAttribute(h, DWMWA_CLOAKED, out cloaked, sizeof(int));
        return cloaked == 0;
    }

    // ---------- hook thread ----------

    static void HookThread() {
        SetThreadDpiAwarenessContext(PER_MONITOR_AWARE_V2);
        hookProc = OnEvent;
        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, IntPtr.Zero, hookProc, 0, 0, WINEVENT_SKIPOWNPROCESS);
        SetWinEventHook(EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND, IntPtr.Zero, hookProc, 0, 0, WINEVENT_SKIPOWNPROCESS);
        MSG msg;
        while (GetMessage(out msg, IntPtr.Zero, 0, 0) > 0) {
            TranslateMessage(ref msg);
            DispatchMessage(ref msg);
        }
    }

    static void OnEvent(IntPtr hook, uint ev, IntPtr hwnd, int idObject, int idChild, uint thread, uint time) {
        try {
            if (idObject != 0 || idChild != 0 || hwnd == IntPtr.Zero) return; // OBJID_WINDOW / CHILDID_SELF only
            uint now = unchecked((uint)Environment.TickCount);
            uint pid;
            GetWindowThreadProcessId(hwnd, out pid);
            if (pid == excludePid) return;

            var o = Describe(hwnd);
            o["ev"] = ev == EVENT_SYSTEM_FOREGROUND ? "foreground" : ev == EVENT_SYSTEM_MOVESIZESTART ? "moveStart" : "moveEnd";
            o["hookLatencyMs"] = (long)unchecked(now - time);
            POINT pt;
            GetCursorPos(out pt);
            o["cursor"] = new int[] { pt.X, pt.Y };

            if (ev == EVENT_SYSTEM_MOVESIZESTART) {
                dragStartFrames[hwnd] = Frame(hwnd);
                o["hit"] = (long)HitTest(hwnd, pt);
            } else if (ev == EVENT_SYSTEM_MOVESIZEEND) {
                RECT start;
                if (dragStartFrames.TryGetValue(hwnd, out start)) {
                    RECT end = Frame(hwnd);
                    o["startFrame"] = R(start);
                    o["sizeChanged"] = (start.Right - start.Left) != (end.Right - end.Left) || (start.Bottom - start.Top) != (end.Bottom - end.Top);
                    dragStartFrames.Remove(hwnd);
                }
                FollowUp(hwnd, 150);
                FollowUp(hwnd, 500);
            }
            o["epochMs"] = EpochMs();
            Emit(o);
        } catch (Exception e) {
            var err = new Dictionary<string, object>();
            err["ev"] = "hookError";
            err["error"] = e.Message;
            Emit(err);
        }
    }

    // MOVESIZESTART fires for both moving and resizing. Asking the window
    // what's under the cursor tells them apart: HTCAPTION (2) is a move,
    // HTLEFT..HTBOTTOMRIGHT (10-17) a resize. The point is converted to the
    // target's own DPI space first, in case it isn't per-monitor aware.
    static int HitTest(IntPtr hwnd, POINT pt) {
        PhysicalToLogicalPointForPerMonitorDPI(hwnd, ref pt);
        IntPtr lp = new IntPtr(unchecked((int)(((uint)(pt.Y & 0xFFFF) << 16) | (uint)(pt.X & 0xFFFF))));
        IntPtr result;
        if (SendMessageTimeout(hwnd, 0x0084, IntPtr.Zero, lp, 0x0002, 100, out result) == IntPtr.Zero) return -1; // WM_NCHITTEST, SMTO_ABORTIFHUNG
        return result.ToInt32();
    }

    // Reports the window's state shortly after a move ends, to catch Aero
    // Snap (or anything else) changing it after MOVESIZEEND has fired.
    static void FollowUp(IntPtr hwnd, int delay) {
        var t = new Thread(delegate() {
            SetThreadDpiAwarenessContext(PER_MONITOR_AWARE_V2);
            Thread.Sleep(delay);
            var o = new Dictionary<string, object>();
            o["ev"] = "afterMove";
            o["hwnd"] = hwnd.ToInt64();
            o["delayMs"] = delay;
            o["zoomed"] = IsZoomed(hwnd);
            o["frame"] = R(Frame(hwnd));
            Emit(o);
        });
        t.IsBackground = true;
        t.Start();
    }

    // ---------- commands ----------

    static long L(Dictionary<string, object> r, string k) { return Convert.ToInt64(r[k]); }
    static IntPtr H(Dictionary<string, object> r) {
        var h = new IntPtr(L(r, "hwnd"));
        if (!IsWindow(h)) throw new Exception("No such window: " + h.ToInt64());
        return h;
    }

    static Dictionary<string, object> Handle(Dictionary<string, object> r) {
        string cmd = (string)r["cmd"];
        var o = new Dictionary<string, object>();
        switch (cmd) {
            case "ping":
                o["epochMs"] = EpochMs();
                return o;
            case "describe":
                return Describe(H(r));
            case "monitors":
                return Monitors();
            case "find":
                return Find(r.ContainsKey("exe") ? (string)r["exe"] : null, r.ContainsKey("pid") ? (uint)L(r, "pid") : 0);
            case "show": {
                var h = H(r);
                ShowWindow(h, (int)L(r, "nCmdShow"));
                return Describe(h);
            }
            case "place":
                return Place(H(r), (int)L(r, "x"), (int)L(r, "y"), (int)L(r, "w"), (int)L(r, "h"));
            case "close":
                PostMessage(H(r), 0x0010, IntPtr.Zero, IntPtr.Zero); // WM_CLOSE
                return o;
            case "cursor": {
                POINT pt;
                GetCursorPos(out pt);
                o["cursor"] = new int[] { pt.X, pt.Y };
                return o;
            }
            case "probe":
                return Probe();
            case "appbarNew": {
                var d = Abd(H(r));
                d.uCallbackMessage = (uint)L(r, "msg");
                o["ok"] = SHAppBarMessage(0, ref d) != UIntPtr.Zero; // ABM_NEW
                return o;
            }
            case "appbarSetPos": {
                var d = Abd(H(r));
                int height = (int)L(r, "height");
                d.uEdge = 1; // ABE_TOP
                d.rc.Left = (int)L(r, "left");
                d.rc.Right = (int)L(r, "right");
                d.rc.Top = (int)L(r, "top");
                d.rc.Bottom = d.rc.Top + height;
                SHAppBarMessage(2, ref d); // ABM_QUERYPOS may push the rect down below other top bars
                o["queried"] = R(d.rc);
                d.rc.Bottom = d.rc.Top + height;
                SHAppBarMessage(3, ref d); // ABM_SETPOS
                o["rect"] = R(d.rc);
                return o;
            }
            case "appbarRemove": {
                var d = Abd(H(r));
                o["ok"] = SHAppBarMessage(1, ref d) != UIntPtr.Zero; // ABM_REMOVE
                return o;
            }
            default:
                throw new Exception("Unknown command: " + cmd);
        }
    }

    static APPBARDATA Abd(IntPtr h) {
        var d = new APPBARDATA();
        d.cbSize = (uint)Marshal.SizeOf(typeof(APPBARDATA));
        d.hWnd = h;
        return d;
    }

    // Moves a window so its visible frame (not its GetWindowRect) fills the
    // target, all in physical pixels. A second pass covers moves to a monitor
    // with a different DPI: the app rescales itself on WM_DPICHANGED and the
    // invisible border changes size, so the first pass can land off target.
    static Dictionary<string, object> Place(IntPtr h, int x, int y, int w, int hgt) {
        var o = new Dictionary<string, object>();
        bool restore = IsZoomed(h) || IsIconic(h);
        if (restore) ShowWindow(h, 9); // SW_RESTORE
        o["restored"] = restore;
        var passes = new List<object>();
        bool ok = false;
        int off = int.MaxValue;
        for (int pass = 1; pass <= 2; pass++) {
            RECT wr;
            GetWindowRect(h, out wr);
            RECT fr = Frame(h);
            int dl = fr.Left - wr.Left, dt = fr.Top - wr.Top, dr = wr.Right - fr.Right, db = wr.Bottom - fr.Bottom;
            ok = SetWindowPos(h, IntPtr.Zero, x - dl, y - dt, w + dl + dr, hgt + dt + db, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
            int err = ok ? 0 : Marshal.GetLastWin32Error();
            RECT after = Frame(h);
            off = Math.Max(Math.Max(Math.Abs(after.Left - x), Math.Abs(after.Top - y)),
                           Math.Max(Math.Abs(after.Right - (x + w)), Math.Abs(after.Bottom - (y + hgt))));
            var p = new Dictionary<string, object>();
            p["borders"] = new int[] { dl, dt, dr, db };
            p["ok"] = ok;
            p["win32Error"] = (long)err;
            p["frameAfter"] = R(after);
            p["maxErrorPx"] = (long)off;
            p["dpiAfter"] = (long)GetDpiForWindow(h);
            passes.Add(p);
            if (!ok || off <= 1) break;
        }
        o["ok"] = ok;
        o["exact"] = ok && off <= 1;
        o["target"] = new int[] { x, y, x + w, y + hgt };
        o["frame"] = R(Frame(h));
        o["passes"] = passes;
        return o;
    }

    static Dictionary<string, object> Monitors() {
        var list = new List<object>();
        MonitorEnumProc cb = delegate(IntPtr hMon, IntPtr hdc, ref RECT rc, IntPtr data) {
            var mi = new MONITORINFOEX();
            mi.cbSize = (uint)Marshal.SizeOf(typeof(MONITORINFOEX));
            GetMonitorInfo(hMon, ref mi);
            uint dx, dy;
            GetDpiForMonitor(hMon, 0, out dx, out dy); // MDT_EFFECTIVE_DPI
            var m = new Dictionary<string, object>();
            m["device"] = mi.szDevice;
            m["bounds"] = R(mi.rcMonitor);
            m["work"] = R(mi.rcWork);
            m["primary"] = (mi.dwFlags & 1) != 0;
            m["dpi"] = (long)dx;
            list.Add(m);
            return true;
        };
        EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, cb, IntPtr.Zero);
        GC.KeepAlive(cb);
        var o = new Dictionary<string, object>();
        o["monitors"] = list;
        return o;
    }

    static Dictionary<string, object> Find(string exe, uint pid) {
        var list = new List<object>();
        EnumWindowsProc cb = delegate(IntPtr h, IntPtr l) {
            if (!IsWindowVisible(h) || GetWindow(h, 4) != IntPtr.Zero) return true;
            uint wpid;
            GetWindowThreadProcessId(h, out wpid);
            if (pid != 0 && wpid != pid) return true;
            if (exe != null && !string.Equals(ExeOf(wpid), exe, StringComparison.OrdinalIgnoreCase)) return true;
            list.Add(Describe(h));
            return true;
        };
        EnumWindows(cb, IntPtr.Zero);
        GC.KeepAlive(cb);
        var o = new Dictionary<string, object>();
        o["windows"] = list;
        return o;
    }

    // true / false, or null when the token can't be read at all.
    static object IsElevated(uint pid) {
        IntPtr p = OpenProcess(0x1000, false, pid); // PROCESS_QUERY_LIMITED_INFORMATION
        if (p == IntPtr.Zero) return null;
        try {
            IntPtr t;
            if (!OpenProcessToken(p, 0x0008, out t)) return null; // TOKEN_QUERY
            try {
                int elevated, len;
                if (!GetTokenInformation(t, 20, out elevated, 4, out len)) return null; // TokenElevation
                return elevated != 0;
            } finally { CloseHandle(t); }
        } finally { CloseHandle(p); }
    }

    // For every window the taskbar would show (not minimized or maximized),
    // re-applies its current rect. Nothing visibly changes; a failure shows
    // which windows UIPI won't let us move (elevated apps).
    static Dictionary<string, object> Probe() {
        var list = new List<object>();
        EnumWindowsProc cb = delegate(IntPtr h, IntPtr l) {
            if (!IsTaskbarWindow(h) || IsIconic(h) || IsZoomed(h)) return true;
            uint wpid;
            GetWindowThreadProcessId(h, out wpid);
            if (wpid == excludePid) return true;
            RECT wr;
            GetWindowRect(h, out wr);
            bool ok = SetWindowPos(h, IntPtr.Zero, wr.Left, wr.Top, wr.Right - wr.Left, wr.Bottom - wr.Top, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
            var w = new Dictionary<string, object>();
            w["exe"] = ExeOf(wpid);
            w["movable"] = ok;
            w["win32Error"] = (long)(ok ? 0 : Marshal.GetLastWin32Error());
            w["elevated"] = IsElevated(wpid);
            list.Add(w);
            return true;
        };
        EnumWindows(cb, IntPtr.Zero);
        GC.KeepAlive(cb);
        var o = new Dictionary<string, object>();
        o["windows"] = list;
        return o;
    }

    // ---------- entry ----------

    public static void Run(uint exclude) {
        excludePid = exclude;
        // Titles can hold any character; the console's OEM code page would
        // mangle them.
        Console.OutputEncoding = new UTF8Encoding(false);
        // Per-thread rather than per-process: powershell.exe may already have
        // a process-wide awareness set, and the thread form works regardless.
        IntPtr prev = SetThreadDpiAwarenessContext(PER_MONITOR_AWARE_V2);
        var hookThread = new Thread(HookThread);
        hookThread.IsBackground = true;
        hookThread.Start();

        var ready = new Dictionary<string, object>();
        ready["ev"] = "ready";
        ready["dpiContextSet"] = prev != IntPtr.Zero;
        ready["epochMs"] = EpochMs();
        Emit(ready);

        string line;
        while ((line = Console.In.ReadLine()) != null) {
            line = line.Trim();
            if (line.Length == 0) continue;
            object id = null;
            try {
                var req = json.Deserialize<Dictionary<string, object>>(line);
                id = req.ContainsKey("id") ? req["id"] : null;
                var res = Handle(req);
                res["id"] = id;
                if (!res.ContainsKey("ok")) res["ok"] = true;
                Emit(res);
            } catch (Exception e) {
                var err = new Dictionary<string, object>();
                err["id"] = id;
                err["ok"] = false;
                err["error"] = e.Message;
                Emit(err);
            }
        }
    }
}
'@

[WmSpike]::Run([uint32]$ExcludePid)
