param(
  [Parameter(Mandatory=$true)][ValidateSet('list','disconnect')]
  [string]$Action,
  [string]$InstanceId
)

switch ($Action) {
  'list' {
    # BTHENUM/BTHLE instance IDs are the actual paired peripherals (headsets,
    # mice, etc.) - as opposed to the 'Bluetooth' PnP class, which also
    # includes the radio/adapter itself and Microsoft's enumerator/RFCOMM/PAN
    # infrastructure entries that aren't real devices a user would recognize.
    $results = @()
    Get-PnpDevice | Where-Object { ($_.InstanceId -like 'BTHENUM*' -or $_.InstanceId -like 'BTHLE*') -and $_.Present } | ForEach-Object {
      $results += [PSCustomObject]@{
        name = $_.FriendlyName
        instanceId = $_.InstanceId
        connected = ($_.Status -eq 'OK')
      }
    }
    if ($results.Count -eq 0) { Write-Output '[]' }
    elseif ($results.Count -eq 1) { Write-Output "[$($results | ConvertTo-Json -Compress)]" }
    else { Write-Output ($results | ConvertTo-Json -Compress) }
  }
  'disconnect' {
    try {
      Disable-PnpDevice -InstanceId $InstanceId -Confirm:$false -ErrorAction Stop
      Write-Output '{"ok":true}'
    } catch {
      $msg = $_.Exception.Message -replace '"','\"'
      Write-Output "{`"ok`":false,`"error`":`"$msg`"}"
    }
  }
}
