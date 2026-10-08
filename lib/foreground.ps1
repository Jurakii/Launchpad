param(
  [Parameter(Mandatory=$true)][string]$ExcludePid
)

# Long-running watcher: polls the foreground window and prints one line
# whenever the answer to "is some other app focused right now?" changes -
# "app" when a real application window is in front, "none" when it's the
# desktop, the taskbar/Start/search shell, nothing at all, or Launchpad
# itself (ExcludePid, so the hotkey can still hide Launchpad). Polling a
# single long-lived process is far cheaper than spawning PowerShell on every
# check. Exits on its own once the Launchpad process that started it is gone.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public class Win32Foreground {
    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr hWnd, StringBuilder lpClassName, int nMaxCount);

    public static string ClassOf(IntPtr hWnd) {
        var sb = new StringBuilder(256);
        GetClassName(hWnd, sb, sb.Capacity);
        return sb.ToString();
    }
}
'@

$shellClasses = @('Progman', 'WorkerW', 'Shell_TrayWnd', 'Shell_SecondaryTrayWnd')
$shellProcesses = @('StartMenuExperienceHost', 'SearchHost', 'SearchApp', 'ShellExperienceHost')
$excluded = [uint32]$ExcludePid

$lastHandle = [IntPtr]::new(-1)
$lastState = ''
$tick = 0

while ($true) {
  $hWnd = [Win32Foreground]::GetForegroundWindow()
  if ($hWnd -ne $lastHandle) {
    $lastHandle = $hWnd
    $state = 'app'
    if ($hWnd -eq [IntPtr]::Zero) {
      $state = 'none'
    } else {
      $procId = [uint32]0
      [void][Win32Foreground]::GetWindowThreadProcessId($hWnd, [ref]$procId)
      if ($procId -eq $excluded) {
        $state = 'none'
      } elseif ($shellClasses -contains [Win32Foreground]::ClassOf($hWnd)) {
        $state = 'none'
      } else {
        try {
          $name = (Get-Process -Id $procId -ErrorAction Stop).ProcessName
          if ($shellProcesses -contains $name) { $state = 'none' }
        } catch {
          # process already gone - keep treating it as an app
        }
      }
    }
    if ($state -ne $lastState) {
      $lastState = $state
      [Console]::Out.WriteLine($state)
      [Console]::Out.Flush()
    }
  }

  $tick++
  if ($tick % 20 -eq 0) {
    try {
      [void][System.Diagnostics.Process]::GetProcessById([int]$excluded)
    } catch {
      exit
    }
  }
  Start-Sleep -Milliseconds 200
}
