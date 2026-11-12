; Positive ACK installer for Windows (NSIS 3, built by build.ps1 with makensis).
;
; Defines passed on the makensis command line (see build.ps1):
;   VERSION            product version, e.g. 0.1.0 (required)
;   PACKIE_EXE            path of packie.exe to install (required)
;   OUTFILE            installer to write (required)
;   CONSOLE_INSTALLER  optional path of the console's Tauri NSIS setup, run silently (/S)
; Not signed.

Unicode true
!include LogicLib.nsh
!ifndef VERSION
  !error "VERSION is not defined"
!endif
!ifndef PACKIE_EXE
  !error "PACKIE_EXE is not defined"
!endif
!ifndef OUTFILE
  !error "OUTFILE is not defined"
!endif

!define PRODUCT "Positive ACK"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\PositiveACK"

Name "${PRODUCT}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\${PRODUCT}"
InstallDirRegKey HKLM "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel admin
ShowInstDetails show
ShowUnInstDetails show

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "FileDescription" "${PRODUCT} installer"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Copyright (C) 2026 BGASoft Inc. and OpenBGA contributors, AGPL-3.0-only"

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

; Runs "packie.exe --bootstrap <file> service <verb>" and aborts the installer or uninstaller with
; the captured output when it fails.
!macro RunService VERB
  nsExec::ExecToStack '"$INSTDIR\packie.exe" --bootstrap "$APPDATA\${PRODUCT}\bootstrap.json" service ${VERB}'
  Pop $0
  Pop $1
  DetailPrint "$1"
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "packie service ${VERB} failed with exit code $0:$\r$\n$\r$\n$1" /SD IDOK
    SetErrorLevel 2
    Abort "packie service ${VERB} failed (exit code $0)"
  ${EndIf}
!macroend


; Machine-wide PATH entry for the install directory and PACKIE_BOOTSTRAP for the default
; bootstrap, so `packie status` works in any new terminal. The Path value is edited in the
; registry unexpanded (other entries keep their %VARIABLES%); setting PACKIE_BOOTSTRAP through
; .NET announces the change to running programs.
!macro SetEnvironment
  nsExec::ExecToStack `powershell -NoProfile -ExecutionPolicy Bypass -Command "$$k = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SYSTEM\CurrentControlSet\Control\Session Manager\Environment', $$true); $$p = $$k.GetValue('Path', '', 'DoNotExpandEnvironmentNames'); $$d = '$INSTDIR'; if (($$p -split ';') -notcontains $$d) { $$k.SetValue('Path', ($$p.TrimEnd(';') + ';' + $$d), 'ExpandString') }; [Environment]::SetEnvironmentVariable('PACKIE_BOOTSTRAP', '$APPDATA\${PRODUCT}\bootstrap.json', 'Machine')"`
  Pop $0
  Pop $1
  ${If} $0 != 0
    DetailPrint "PATH and PACKIE_BOOTSTRAP not set (exit code $0): $1"
  ${Else}
    DetailPrint "Added $INSTDIR to PATH and set PACKIE_BOOTSTRAP for new terminals"
  ${EndIf}
!macroend

!macro ClearEnvironment
  nsExec::ExecToStack `powershell -NoProfile -ExecutionPolicy Bypass -Command "$$k = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SYSTEM\CurrentControlSet\Control\Session Manager\Environment', $$true); $$p = $$k.GetValue('Path', '', 'DoNotExpandEnvironmentNames'); $$d = '$INSTDIR'; $$k.SetValue('Path', ((($$p -split ';') | Where-Object { $$_ -and $$_ -ne $$d }) -join ';'), 'ExpandString'); [Environment]::SetEnvironmentVariable('PACKIE_BOOTSTRAP', $$null, 'Machine')"`
  Pop $0
  Pop $1
!macroend

Section "Install"
  SetShellVarContext all   ; $APPDATA is C:\ProgramData
  SetRegView 64

  ; Upgrade: remove the previous service registration so packie.exe is not locked.
  ${If} ${FileExists} "$INSTDIR\packie.exe"
    DetailPrint "Removing the previous service"
    nsExec::ExecToStack '"$INSTDIR\packie.exe" --bootstrap "$APPDATA\${PRODUCT}\bootstrap.json" service uninstall'
    Pop $0
    Pop $1
    ${If} $0 != 0
      DetailPrint "Previous service not removed (exit code $0); continuing: $1"
    ${EndIf}
  ${EndIf}

  SetOutPath "$INSTDIR"
  File "${PACKIE_EXE}"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Default bootstrap: never overwrite an operator's file.
  ${IfNot} ${FileExists} "$APPDATA\${PRODUCT}\bootstrap.json"
    CreateDirectory "$APPDATA\${PRODUCT}"
    SetOutPath "$APPDATA\${PRODUCT}"
    File /oname=bootstrap.json "bootstrap.json"
    SetOutPath "$INSTDIR"
  ${EndIf}

  DetailPrint "Installing the ${PRODUCT} service"
  !insertmacro RunService install
  !insertmacro SetEnvironment

!ifdef CONSOLE_INSTALLER
  DetailPrint "Installing the console"
  InitPluginsDir
  File /oname=$PLUGINSDIR\console-setup.exe "${CONSOLE_INSTALLER}"
  ExecWait '"$PLUGINSDIR\console-setup.exe" /S' $0
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONEXCLAMATION "The console installer exited with code $0. The service is installed; install the console separately." /SD IDOK
    SetErrorLevel 3
  ${EndIf}
!endif

  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "Publisher" "Positive ACK contributors"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  SetShellVarContext all
  SetRegView 64

  DetailPrint "Removing the ${PRODUCT} service"
  !insertmacro RunService uninstall

  !insertmacro ClearEnvironment
  Delete "$INSTDIR\packie.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "${UNINSTALL_KEY}"

  DetailPrint "Kept $APPDATA\${PRODUCT} (bootstrap, state and logs). Delete it by hand to remove all data."
SectionEnd
