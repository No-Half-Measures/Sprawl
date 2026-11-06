Unicode true
!ifndef PRODUCTNAME
!define PRODUCTNAME "Sprawl"
!endif
!ifndef BUNDLEID
!define BUNDLEID "com.sprawl.app"
!endif
!ifndef MANUFACTURER
!define MANUFACTURER "sprawl"
!endif
!ifndef VERSION
!define VERSION "0.2.0"
!endif
!ifndef BINDIR
!define BINDIR "..\..\target\release"
!endif
!ifndef OUTDIR
!define OUTDIR "..\..\target\installer"
!endif
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
!define PRODUCTKEY "Software\${MANUFACTURER}\${PRODUCTNAME}"
!define MUI_ICON "..\..\crates\sprawl\assets\icon.ico"
!define MUI_UNICON "..\..\crates\sprawl\assets\icon.ico"
!define MULTIUSER_MUI
!define MULTIUSER_EXECUTIONLEVEL Highest
!define MULTIUSER_INSTALLMODE_COMMANDLINE
!define MULTIUSER_INSTALLMODE_INSTDIR "${PRODUCTNAME}"
!define MULTIUSER_USE_PROGRAMFILES64
!define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_KEY "${UNINSTKEY}"
!define MULTIUSER_INSTALLMODE_DEFAULT_REGISTRY_VALUENAME "CurrentUser"
!define MULTIUSER_INSTALLMODE_FUNCTION RestoreDirectory
!include MUI2.nsh
!include MultiUser.nsh
!include LogicLib.nsh
!include FileFunc.nsh
!include nsDialogs.nsh
!include "hooks.nsh"

Name "${PRODUCTNAME}"
OutFile "${OUTDIR}\${PRODUCTNAME}_${VERSION}_x64-setup.exe"
!define DEFAULT_DIRECTORY "placeholder\${PRODUCTNAME}"
InstallDir "${DEFAULT_DIRECTORY}"
ShowInstDetails show
ShowUninstDetails show
Var UpdateMode
Var DeleteData
Var DeleteCheckbox
Var RequestedDir

!insertmacro MUI_PAGE_WELCOME
!insertmacro MULTIUSER_PAGE_INSTALLMODE
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_FUNCTION RunApp
!define MUI_FINISHPAGE_RUN_TEXT "Run ${PRODUCTNAME}"
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Create a desktop shortcut"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION DesktopShortcut
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED
!insertmacro MUI_PAGE_FINISH
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.ConfirmShow
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE un.ConfirmLeave
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function RestoreDirectory
  ${If} $RequestedDir != ""
    StrCpy $INSTDIR $RequestedDir
    Return
  ${EndIf}
  ${If} $MultiUser.InstallMode == "CurrentUser"
    StrCpy $INSTDIR "$LOCALAPPDATA\${PRODUCTNAME}"
  ${Else}
    StrCpy $INSTDIR "$PROGRAMFILES64\${PRODUCTNAME}"
  ${EndIf}
  ReadRegStr $0 SHCTX "${PRODUCTKEY}" ""
  ${If} $0 != ""
    StrCpy $INSTDIR $0
  ${EndIf}
FunctionEnd

Function .onInit
  SetRegView 64
  StrCpy $RequestedDir $INSTDIR
  ${If} $RequestedDir == "${DEFAULT_DIRECTORY}"
    StrCpy $RequestedDir ""
  ${EndIf}
  !insertmacro MULTIUSER_INIT
FunctionEnd

; OpenMutex can fail for an elevated instance; access denied still means running.
!macro CheckRunning PREFIX
Function ${PREFIX}CheckRunning
retry:
  System::Call 'kernel32::OpenMutexW(i 0x100000, i 0, w "${BUNDLEID}-sim") p.r0 ?e'
  Pop $1
  ${If} $0 P<> 0
    System::Call 'kernel32::CloseHandle(p r0)'
    Goto running
  ${EndIf}
  ${If} $1 != 5
    Return
  ${EndIf}
running:
  IfSilent fail
  MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "Close ${PRODUCTNAME}, then choose Retry." IDRETRY retry
fail:
  SetErrorLevel 1
  Quit
FunctionEnd
!macroend
!insertmacro CheckRunning ""
!insertmacro CheckRunning "un."

Function RunApp
  !ifndef INSTALLER_TEST
    Exec '"$WINDIR\explorer.exe" "$INSTDIR\sprawl.exe"'
  !endif
FunctionEnd
Function DesktopShortcut
  CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\sprawl.exe"
FunctionEnd

Section "Install"
  Call CheckRunning
  ReadRegStr $0 SHCTX "${UNINSTKEY}" "UninstallString"
  ${If} $0 != ""
    ReadRegStr $1 SHCTX "${PRODUCTKEY}" ""
    ${If} $1 == ""
      ReadRegStr $1 SHCTX "${UNINSTKEY}" "InstallLocation"
      ; Strip the quotes used by both the Tauri installer and this installer.
      StrCpy $2 $1 1
      ${If} $2 == '$\"'
        StrCpy $1 $1 -1 1
      ${EndIf}
    ${EndIf}
    ${If} $1 == ""
      SetErrorLevel 2
      Abort "Previous installation directory is missing."
    ${EndIf}
    ClearErrors
    ExecWait '$0 /S /UPDATE _?=$1' $2
    ${If} ${Errors}
    ${OrIf} $2 != 0
      SetErrorLevel 3
      Abort "Previous uninstaller failed."
    ${EndIf}
    Delete "$1\uninstall.exe"
  ${EndIf}
  SetShellVarContext current
  RMDir /r "$LOCALAPPDATA\${BUNDLEID}\EBWebView"
  ${If} $MultiUser.InstallMode == "AllUsers"
    SetShellVarContext all
  ${EndIf}
  SetOutPath "$INSTDIR"
  File "${BINDIR}\sprawl.exe"
  File "${BINDIR}\sprawl-cli.exe"
  File "..\..\LICENSE"
  File "..\..\NOTICE"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr SHCTX "${PRODUCTKEY}" "" "$INSTDIR"
  WriteRegStr SHCTX "${UNINSTKEY}" "$MultiUser.InstallMode" "1"
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\sprawl.exe"
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr SHCTX "${UNINSTKEY}" "Publisher" "${MANUFACTURER}"
  WriteRegStr SHCTX "${UNINSTKEY}" "InstallLocation" '$\"$INSTDIR$\"'
  WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" '$\"$INSTDIR\uninstall.exe$\"'
  WriteRegStr SHCTX "${UNINSTKEY}" "MainBinaryName" "sprawl.exe"
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD SHCTX "${UNINSTKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD SHCTX "${UNINSTKEY}" "EstimatedSize" $0
  CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\sprawl.exe"
  !insertmacro SPRAWL_EDIT_PATH "add"
SectionEnd

Function un.onInit
  SetRegView 64
  !insertmacro MULTIUSER_UNINIT
  ReadRegStr $0 HKLM "${PRODUCTKEY}" ""
  ${If} $0 == $INSTDIR
    Call un.MultiUser.InstallMode.AllUsers
  ${Else}
    Call un.MultiUser.InstallMode.CurrentUser
  ${EndIf}
  StrCpy $UpdateMode 0
  StrCpy $DeleteData 0
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/UPDATE" $1
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
FunctionEnd

Function un.ConfirmShow
  ${If} $UpdateMode == 1
    Return
  ${EndIf}
  ; MUI's confirm page is a native dialog, so create a stock checkbox in it.
  FindWindow $0 "#32770" "" $HWNDPARENT
  System::Call 'user32::CreateWindowExW(i 0, w "Button", w "Delete saved scans and settings", i 0x50010003, i 0, i 140, i 320, i 24, p r0, p 0, p 0, p 0) p .r1'
  StrCpy $DeleteCheckbox $1
  SendMessage $0 ${WM_GETFONT} 0 0 $2
  SendMessage $DeleteCheckbox ${WM_SETFONT} $2 1
FunctionEnd
Function un.ConfirmLeave
  ${If} $DeleteCheckbox P<> 0
    SendMessage $DeleteCheckbox ${BM_GETCHECK} 0 0 $DeleteData
  ${EndIf}
FunctionEnd

Section "Uninstall"
  Call un.CheckRunning
  !insertmacro SPRAWL_EDIT_PATH "remove"
  Delete "$INSTDIR\sprawl.exe"
  Delete "$INSTDIR\sprawl-cli.exe"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\NOTICE"
  ${If} $UpdateMode != 1
    Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    Delete "$DESKTOP\${PRODUCTNAME}.lnk"
    DeleteRegKey SHCTX "${UNINSTKEY}"
    DeleteRegKey SHCTX "${PRODUCTKEY}"
    DeleteRegKey /ifempty SHCTX "Software\${MANUFACTURER}"
    !ifndef INSTALLER_TEST
      DeleteRegKey HKCU "Software\Classes\Directory\shell\${PRODUCTNAME}"
      DeleteRegKey HKCU "Software\Classes\Directory\Background\shell\${PRODUCTNAME}"
      DeleteRegKey HKCU "Software\Classes\Drive\shell\${PRODUCTNAME}"
    !endif
    ${If} $DeleteData == ${BST_CHECKED}
    ${AndIfNot} ${Silent}
      SetShellVarContext current
      RMDir /r "$LOCALAPPDATA\${BUNDLEID}"
    ${EndIf}
  ${EndIf}
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
