param(
  [Parameter(Mandatory=$true)][ValidateSet('list','eject')]
  [string]$Action,
  [string]$DriveLetter
)

switch ($Action) {
  'list' {
    $results = @()
    Get-Volume | Where-Object { $_.DriveLetter } | ForEach-Object {
      $vol = $_
      $busType = 'Unknown'
      try {
        $partition = Get-Partition -DriveLetter $vol.DriveLetter -ErrorAction Stop
        $disk = Get-Disk -Number $partition.DiskNumber -ErrorAction Stop
        $busType = $disk.BusType.ToString()
      } catch {}
      # USB-connected storage only - flash drives and external hard drives both
      # report BusType 'USB' here; internal SATA/NVMe drives are excluded.
      if ($busType -eq 'USB') {
        $results += [PSCustomObject]@{
          driveLetter = "$($vol.DriveLetter):"
          label = $vol.FileSystemLabel
          size = [int64]$vol.Size
          freeSpace = [int64]$vol.SizeRemaining
        }
      }
    }
    if ($results.Count -eq 0) {
      Write-Output '[]'
    } elseif ($results.Count -eq 1) {
      Write-Output "[$($results | ConvertTo-Json -Compress)]"
    } else {
      Write-Output ($results | ConvertTo-Json -Compress)
    }
  }
  'eject' {
    try {
      $shell = New-Object -ComObject Shell.Application
      # 17 = ssfDRIVES ("My Computer"); InvokeVerb("Eject") is the scripted
      # equivalent of right-click > Eject in Explorer.
      $item = $shell.Namespace(17).ParseName($DriveLetter)
      if ($null -eq $item) {
        Write-Output '{"ok":false,"error":"Drive not found"}'
      } else {
        $item.InvokeVerb('Eject')
        Start-Sleep -Milliseconds 800
        Write-Output '{"ok":true}'
      }
    } catch {
      $msg = $_.Exception.Message -replace '"','\"'
      Write-Output "{`"ok`":false,`"error`":`"$msg`"}"
    }
  }
}
