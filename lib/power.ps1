param(
  [Parameter(Mandatory=$true)][ValidateSet('shutdown','restart','sleep')]
  [string]$Action
)

switch ($Action) {
  'shutdown' { shutdown.exe /s /t 0 }
  'restart' { shutdown.exe /r /t 0 }
  'sleep' { rundll32.exe powrprof.dll,SetSuspendState 0,1,0 }
}
