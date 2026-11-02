; Puts the install folder, which holds sprawl-cli.exe, on PATH and takes it off
; again before uninstall. Per-machine installs edit the system PATH, per-user
; installs the user PATH. PowerShell edits the registry because NSIS strings are
; capped at 1024 characters and would truncate a long PATH.
!include WinMessages.nsh

!define SPRAWL_PATH_SCRIPT "\
$$d = $$env:SPRAWL_PATH_DIR.TrimEnd('\');\
$$k = if ($$env:SPRAWL_PATH_SCOPE -eq 'Machine') {\
[Microsoft.Win32.Registry]::LocalMachine.CreateSubKey('SYSTEM\CurrentControlSet\Control\Session Manager\Environment')\
} else { [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment') };\
$$p = @([string]$$k.GetValue('Path', '', 'DoNotExpandEnvironmentNames') -split ';' |\
Where-Object { $$_ -and $$_.TrimEnd('\') -ne $$d });\
if ($$env:SPRAWL_PATH_ACTION -eq 'add') { $$p += $$d };\
if ($$p) { $$k.SetValue('Path', $$p -join ';', 'ExpandString') } else { $$k.DeleteValue('Path', $$false) }"

!macro SPRAWL_EDIT_PATH ACTION
  Push $0
  ${If} ${ShellVarContextAll}
    StrCpy $0 "Machine"
  ${Else}
    StrCpy $0 "User"
  ${EndIf}
  System::Call 'kernel32::SetEnvironmentVariable(t "SPRAWL_PATH_SCOPE", t "$0")'
  System::Call 'kernel32::SetEnvironmentVariable(t "SPRAWL_PATH_DIR", t "$INSTDIR")'
  System::Call 'kernel32::SetEnvironmentVariable(t "SPRAWL_PATH_ACTION", t "${ACTION}")'
  nsExec::ExecToLog `"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "${SPRAWL_PATH_SCRIPT}"`
  Pop $0
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
  Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro SPRAWL_EDIT_PATH "add"
!macroend

; Tauri's uninstaller can switch to the current-user context before the
; post-uninstall hook, so remove the PATH entry while the scope is still right.
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro SPRAWL_EDIT_PATH "remove"
!macroend
