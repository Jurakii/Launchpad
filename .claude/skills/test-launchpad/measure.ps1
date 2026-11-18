# Prints the memory of a running Launchpad (every process in its tree) and,
# optionally, the size of an exe, install folder or installer, as JSON.
#   measure.ps1                      the installed or Rust build (Launchpad.exe)
#   measure.ps1 -Name electron       the Electron dev build (npm start)
#   measure.ps1 -Path dist\Launchpad-Setup-2.0.0.exe,target\release\launchpad.exe
param(
  [string]$Name = 'Launchpad',
  [string[]]$Path = @()
)

# powershell -File hands "a,b" over as one string.
$Path = @($Path | ForEach-Object { $_ -split ',' } | Where-Object { $_ })

$out = [ordered]@{}
$roots = @(Get-Process -Name $Name -ErrorAction SilentlyContinue)
if ($roots.Count -gt 0) {
  # Helpers (PowerShell, conhost) are children, so walk the tree rather than match names.
  $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId
  $ids = New-Object 'System.Collections.Generic.HashSet[int]'
  foreach ($r in $roots) { [void]$ids.Add($r.Id) }
  do {
    $added = $false
    foreach ($p in $all) {
      if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $added = $true }
    }
  } while ($added)
  $procs = @(Get-Process -Id @($ids) -ErrorAction SilentlyContinue)
  $out.processes = @($procs | ForEach-Object {
    [ordered]@{ name = $_.ProcessName; id = $_.Id
      privateMB = [math]::Round($_.PrivateMemorySize64 / 1MB, 1)
      workingSetMB = [math]::Round($_.WorkingSet64 / 1MB, 1) }
  })
  # Private bytes is the number the budgets use: working sets count shared pages once per process.
  $out.totalPrivateMB = [math]::Round(($procs | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB, 1)
  $out.totalWorkingSetMB = [math]::Round(($procs | Measure-Object WorkingSet64 -Sum).Sum / 1MB, 1)
  $out.processCount = $procs.Count
} else {
  $out.error = "No running process named '$Name'"
}

if ($Path.Count -gt 0) {
  $out.sizes = @($Path | ForEach-Object {
    if (Test-Path $_) {
      # A file on its own: -Recurse on a file path lists it twice in Windows PowerShell.
      $bytes = if (Test-Path $_ -PathType Leaf) { (Get-Item $_ -Force).Length } else { (Get-ChildItem $_ -Recurse -File -Force | Measure-Object Length -Sum).Sum }
      [ordered]@{ path = $_; MB = [math]::Round($bytes / 1MB, 2) }
    } else {
      [ordered]@{ path = $_; error = 'not found' }
    }
  })
}

$out | ConvertTo-Json -Depth 4
