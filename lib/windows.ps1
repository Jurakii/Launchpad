param(
  [Parameter(Mandatory=$true)][ValidateSet('list','focus')]
  [string]$Action,
  [string]$Handle,
  [string]$ExcludePid
)

# The naive "Get-Process | Where MainWindowHandle" approach picks up windows
# that are never actually shown on the real Windows taskbar - most visibly,
# UWP apps (Media Player etc.) run inside an ApplicationFrameHost.exe that
# keeps a second, DWM-cloaked host window alive in the background, which
# still reports a valid handle/title. This instead applies the same checks
# the taskbar itself effectively uses: visible, top-level (no owner), not a
# tool window, and not DWM-cloaked.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public class Win32Window {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")]
    public static extern int GetWindowTextLength(IntPtr hWnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr hWnd, StringBuilder lpString, int nMaxCount);
    [DllImport("user32.dll")]
    public static extern IntPtr GetWindow(IntPtr hWnd, uint uCmd);
    [DllImport("user32.dll")]
    public static extern int GetWindowLong(IntPtr hWnd, int nIndex);
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("dwmapi.dll")]
    public static extern int DwmGetWindowAttribute(IntPtr hwnd, int dwAttribute, out int pvAttribute, int cbAttribute);
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")]
    public static extern bool IsIconic(IntPtr hWnd);

    const uint GW_OWNER = 4;
    const int GWL_EXSTYLE = -20;
    const int WS_EX_TOOLWINDOW = 0x00000080;
    const int DWMWA_CLOAKED = 14;

    public class WinInfo {
        public IntPtr Handle;
        public string Title;
        public uint Pid;
    }

    public static List<WinInfo> ListTaskbarWindows() {
        var results = new List<WinInfo>();
        EnumWindows((hWnd, lParam) => {
            if (!IsWindowVisible(hWnd)) return true;
            if (GetWindowTextLength(hWnd) == 0) return true;
            if (GetWindow(hWnd, GW_OWNER) != IntPtr.Zero) return true;
            if ((GetWindowLong(hWnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW) != 0) return true;

            int cloaked = 0;
            DwmGetWindowAttribute(hWnd, DWMWA_CLOAKED, out cloaked, sizeof(int));
            if (cloaked != 0) return true;

            var sb = new StringBuilder(512);
            GetWindowText(hWnd, sb, sb.Capacity);
            string title = sb.ToString();
            if (string.IsNullOrWhiteSpace(title)) return true;

            uint pid;
            GetWindowThreadProcessId(hWnd, out pid);
            results.Add(new WinInfo { Handle = hWnd, Title = title, Pid = pid });
            return true;
        }, IntPtr.Zero);
        return results;
    }
}
'@

switch ($Action) {
  'list' {
    $excludePidNum = 0
    if ($ExcludePid) { $excludePidNum = [int]$ExcludePid }

    $results = @()
    foreach ($win in [Win32Window]::ListTaskbarWindows()) {
      if ($win.Pid -eq $excludePidNum) { continue }
      $exePath = $null
      try { $exePath = (Get-Process -Id $win.Pid -ErrorAction Stop).Path } catch {}
      $results += [PSCustomObject]@{
        pid = $win.Pid
        title = $win.Title
        handle = [int64]$win.Handle
        path = $exePath
      }
    }
    if ($results.Count -eq 0) { Write-Output '[]' }
    elseif ($results.Count -eq 1) { Write-Output "[$($results | ConvertTo-Json -Compress)]" }
    else { Write-Output ($results | ConvertTo-Json -Compress) }
  }
  'focus' {
    try {
      $hwnd = [IntPtr]([int64]$Handle)
      if ([Win32Window]::IsIconic($hwnd)) { [Win32Window]::ShowWindow($hwnd, 9) | Out-Null } # SW_RESTORE
      [Win32Window]::SetForegroundWindow($hwnd) | Out-Null
      Write-Output '{"ok":true}'
    } catch {
      $msg = $_.Exception.Message -replace '"','\"'
      Write-Output "{`"ok`":false,`"error`":`"$msg`"}"
    }
  }
}
