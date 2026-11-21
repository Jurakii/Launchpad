; Launchpad 1.3 installer (RUST_PORT_PLAN.md §4.4, Phase 6).
;
;   makensis /DVERSION=1.3.0 /DEXE=target\release\launchpad.exe /DOUT=dist\Launchpad-Setup-1.3.0.exe installer\launchpad.nsi
;
; Per-machine, in Program Files, as 1.2.2 was installed. Over a 1.2.x
; (Electron) install it quits Launchpad, runs the old uninstaller silently
; (files only: electron-builder never deletes the data folder unless told to)
; and installs to the same place. Over 1.3 it just replaces the files.
; Nothing here touches %APPDATA%\launchpad, installing or uninstalling.
;
; Switches: /S silent. --force-run starts Launchpad when done, which is how
; the in-app updater runs it (electron-builder's name for the same thing).

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel admin
ManifestDPIAware true

!ifndef VERSION
  !error "Pass /DVERSION=x.y.z"
!endif
!ifndef EXE
  !define EXE "..\target\release\launchpad.exe"
!endif
!ifndef OUT
  !define OUT "..\dist\Launchpad-Setup-${VERSION}.exe"
!endif

; electron-builder's key for appId com.ncrouse.launchpad, kept so Apps &
; Features shows one Launchpad across the switch.
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\ff3d7479-69db-544e-9908-b32d3ad9390a"
!define ICON "..\build\icon.ico"

Name "Launchpad"
OutFile "${OUT}"
InstallDir "$PROGRAMFILES64\Launchpad"
BrandingText "Launchpad ${VERSION}"
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "Launchpad"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "Launchpad Setup"
VIAddVersionKey "LegalCopyright" "Jurakii"

!include "LogicLib.nsh"
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "WordFunc.nsh"
!include "x64.nsh"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Start Launchpad"
!define MUI_FINISHPAGE_RUN_FUNCTION StartApp
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Asks Launchpad.exe (1.2's Electron processes, or 1.3) to go, waits up to
; 5 s (the in-app updater is quitting by itself), then ends what's left.
!macro CloseLaunchpad
  StrCpy $R9 0
  ${Do}
    nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq Launchpad.exe" /NH | find /I "Launchpad.exe"'
    Pop $0
    Pop $1
    ${If} $0 != 0
      ${Break}
    ${EndIf}
    ${If} $R9 >= 10
      nsExec::Exec 'taskkill /F /T /IM Launchpad.exe'
      Sleep 1000
      ${Break}
    ${EndIf}
    ${If} $R9 = 0
      nsExec::Exec 'taskkill /IM Launchpad.exe'
    ${EndIf}
    Sleep 500
    IntOp $R9 $R9 + 1
  ${Loop}
!macroend

; If $0 (a QuietUninstallString) is electron-builder's uninstaller, runs it
; and waits for it (_?= keeps it from copying itself to temp and returning
; at once), then deletes the uninstaller it leaves behind.
Function RunElectronUninstaller
  ${If} $0 == ""
    Return
  ${EndIf}
  ${WordFind} $0 '"' "+1" $1 ; the quoted path
  ${GetFileName} $1 $2
  ${If} $2 != "Uninstall Launchpad.exe"
    Return
  ${EndIf}
  ${GetParent} $1 $3
  DetailPrint "Removing Launchpad 1.2 from $3"
  ExecWait '$0 --updated _?=$3'
  Delete "$1"
  RMDir "$3"
FunctionEnd

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "Launchpad needs 64-bit Windows." /SD IDOK
    Abort
  ${EndIf}
  SetRegView 64
  ; An earlier install's folder wins, so an update stays where it was.
  ReadRegStr $0 HKLM "${UNINST_KEY}" "InstallLocation"
  ${If} $0 != ""
    StrCpy $INSTDIR $0
  ${Else}
    ReadRegStr $0 HKLM "${UNINST_KEY}" "QuietUninstallString"
    ${WordFind} $0 '"' "+1" $1
    ${If} ${FileExists} $1
      ${GetParent} $1 $INSTDIR
    ${EndIf}
  ${EndIf}
FunctionEnd

Function StartApp
  ; Through Explorer so Launchpad runs as the signed-in user, not elevated
  ; like this installer.
  Exec '"$WINDIR\explorer.exe" "$INSTDIR\Launchpad.exe"'
FunctionEnd

Section "Launchpad"
  SetRegView 64
  SetShellVarContext all
  !insertmacro CloseLaunchpad

  ; 1.2.x, per-machine and per-user.
  ReadRegStr $0 HKLM "${UNINST_KEY}" "QuietUninstallString"
  Call RunElectronUninstaller
  ReadRegStr $0 HKCU "${UNINST_KEY}" "QuietUninstallString"
  Call RunElectronUninstaller
  DeleteRegKey HKCU "${UNINST_KEY}"

  SetOutPath "$INSTDIR"
  File "/oname=Launchpad.exe" "${EXE}"
  ; 1.3.0 shipped the icon beside the exe; it's embedded in the exe now.
  Delete "$INSTDIR\build\icon.ico"
  RMDir "$INSTDIR\build"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateShortcut "$SMPROGRAMS\Launchpad.lnk" "$INSTDIR\Launchpad.exe"
  CreateShortcut "$DESKTOP\Launchpad.lnk" "$INSTDIR\Launchpad.exe"

  WriteRegStr HKLM "${UNINST_KEY}" "DisplayName" "Launchpad"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINST_KEY}" "Publisher" "Jurakii"
  WriteRegStr HKLM "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\Launchpad.exe,0"
  WriteRegStr HKLM "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKLM "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKLM "${UNINST_KEY}" "EstimatedSize" $0

  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "--force-run" $1
  ${IfNot} ${Errors}
    Call StartApp
  ${EndIf}
SectionEnd

Section "Uninstall"
  SetRegView 64
  SetShellVarContext all
  !insertmacro CloseLaunchpad
  Delete "$INSTDIR\Launchpad.exe"
  Delete "$INSTDIR\build\icon.ico"
  RMDir "$INSTDIR\build"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Launchpad.lnk"
  Delete "$DESKTOP\Launchpad.lnk"
  DeleteRegKey HKLM "${UNINST_KEY}"
  ; "Start with Windows" (the app writes it per user).
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Launchpad"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Launchpad"
SectionEnd
