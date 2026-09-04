param(
  [Parameter(Mandatory=$true)][ValidateSet('list','connect','disconnect')]
  [string]$Action,
  [string]$Ssid
)

# netsh has no JSON output mode, so its text output is parsed by hand below.

function Get-CurrentSsid {
  $output = netsh wlan show interfaces
  foreach ($line in $output) {
    if ($line -match '^\s*SSID\s*:\s*(.+)$') {
      return $matches[1].Trim()
    }
  }
  return $null
}

function Get-SavedProfiles {
  $output = netsh wlan show profiles
  $profiles = @()
  foreach ($line in $output) {
    if ($line -match '^\s*All User Profile\s*:\s*(.+)$') {
      $profiles += $matches[1].Trim()
    }
  }
  return $profiles
}

switch ($Action) {
  'list' {
    $currentSsid = Get-CurrentSsid
    $savedProfiles = Get-SavedProfiles
    $output = netsh wlan show networks
    $results = @()
    $currentName = $null
    foreach ($line in $output) {
      if ($line -match '^SSID\s+\d+\s*:\s*(.*)$') {
        $currentName = $matches[1].Trim()
      } elseif ($line -match '^\s*Authentication\s*:\s*(.+)$' -and $null -ne $currentName) {
        $auth = $matches[1].Trim()
        if ($currentName -ne '') {
          $results += [PSCustomObject]@{
            ssid = $currentName
            auth = $auth
            connected = ($currentName -eq $currentSsid)
            saved = ($savedProfiles -contains $currentName)
          }
        }
        $currentName = $null
      }
    }
    if ($results.Count -eq 0) { Write-Output '[]' }
    elseif ($results.Count -eq 1) { Write-Output "[$($results | ConvertTo-Json -Compress)]" }
    else { Write-Output ($results | ConvertTo-Json -Compress) }
  }
  'connect' {
    try {
      $out = (netsh wlan connect name="$Ssid" ssid="$Ssid" 2>&1) -join ' '
      if ($out -match 'successfully' -or $out -match 'was completed successfully' -or $out -match 'Connection request') {
        Write-Output '{"ok":true}'
      } else {
        $msg = $out -replace '"','\"'
        Write-Output "{`"ok`":false,`"error`":`"$msg`"}"
      }
    } catch {
      $msg = $_.Exception.Message -replace '"','\"'
      Write-Output "{`"ok`":false,`"error`":`"$msg`"}"
    }
  }
  'disconnect' {
    try {
      netsh wlan disconnect | Out-Null
      Write-Output '{"ok":true}'
    } catch {
      $msg = $_.Exception.Message -replace '"','\"'
      Write-Output "{`"ok`":false,`"error`":`"$msg`"}"
    }
  }
}
