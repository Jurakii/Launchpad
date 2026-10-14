# Gives back the space Launchpad's organizer panels reserve if Launchpad
# dies without doing it itself. Node puts every process it starts in a job
# that Windows kills along with Launchpad, so after a crash windowmgr.ps1
# is gone before it can remove its AppBars, and Explorer keeps the top of
# each screen reserved. windowmgr.ps1 starts this process itself, which keeps
# it out of that job (Node's job lets its processes' children leave it). It
# tells this one about each AppBar on stdin, "add <hwnd>" or "remove <hwnd>",
# and when stdin closes - the helper exited, cleanly or not - any that are
# left are removed. Explorer only needs the handle, so it doesn't matter that
# the windows are gone by then.
$open = New-Object 'System.Collections.Generic.HashSet[long]'
while ($null -ne ($line = [Console]::In.ReadLine())) {
  $parts = $line.Trim().Split(' ')
  if ($parts.Length -ne 2) { continue }
  $hwnd = 0L
  if (-not [long]::TryParse($parts[1], [ref]$hwnd)) { continue }
  if ($parts[0] -eq 'add') { [void]$open.Add($hwnd) }
  elseif ($parts[0] -eq 'remove') { [void]$open.Remove($hwnd) }
}
if ($open.Count -eq 0) { exit }

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class AppBarGuard {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct APPBARDATA { public uint cbSize; public IntPtr hWnd; public uint uCallbackMessage; public uint uEdge; public RECT rc; public IntPtr lParam; }

    [DllImport("shell32.dll")] static extern UIntPtr SHAppBarMessage(uint msg, ref APPBARDATA data);

    public static void Remove(long hwnd) {
        var d = new APPBARDATA();
        d.cbSize = (uint)Marshal.SizeOf(typeof(APPBARDATA));
        d.hWnd = new IntPtr(hwnd);
        SHAppBarMessage(1, ref d); // ABM_REMOVE
    }
}
'@
foreach ($hwnd in $open) { [AppBarGuard]::Remove($hwnd) }
