; config-digger's additions to Tauri's NSIS installer, which includes this
; file before its own pages (`bundle.windows.nsis.installerHooks`).
;
; - The welcome page carries a checkbox, checked by default, that puts the
;   install folder on the user PATH (HKCU). The uninstaller takes it off.
;   An unticked box is remembered for the next install, silent ones included.
;   `$PROFILE` is never touched. The PATH is edited by the console exe,
;   `config-digger installer add-to-path|remove-from-path`, because NSIS
;   strings stop at 1024 characters and would cut a long PATH.
; - The uninstaller's "Delete config-digger data" box (Tauri's
;   `$DeleteAppDataCheckboxState`, unchecked by default) also deletes
;   config-digger's data folders, after a warning that names the parked
;   Entries (`config-digger installer parked`). It never restores them.
;
; An update (`/UPDATE`) leaves the PATH and the data as they are.

!include LogicLib.nsh
!include nsDialogs.nsh
!include WinMessages.nsh

!define CLI_EXE "config-digger.exe"

; config-digger's own data folders, as the core finds them.
!define DATA_LOCAL "$LOCALAPPDATA\config-digger"
!define DATA_ROAMING "$APPDATA\config-digger"

; Where the PATH choice is remembered: Tauri's `${MANUPRODUCTKEY}` for the
; publisher samfaina, not defined yet where this file is included. Tauri
; deletes it with config-digger's data.
!define PATH_CHOICE_KEY "Software\samfaina\config-digger"
!define PATH_CHOICE_VALUE "KeepPathAlone"

Var AddToPathCheckbox
; 1 when the user unticked the checkbox, now or at the last install; 0 or
; empty, so ticked, otherwise.
Var KeepPathAlone

Function LoadPathChoice
  ${If} $KeepPathAlone == ""
    ReadRegDWORD $KeepPathAlone HKCU "${PATH_CHOICE_KEY}" "${PATH_CHOICE_VALUE}"
  ${EndIf}
FunctionEnd

; Tauri's welcome page is the first page inserted after this file.
!define MUI_PAGE_CUSTOMFUNCTION_SHOW AddToPathShow
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE AddToPathLeave

; Below the welcome text, which ends at 175u, on the page's white: MUI's
; default `MUI_BGCOLOR`, not defined yet where this file is included.
Function AddToPathShow
  Call LoadPathChoice
  ${NSD_CreateCheckbox} 120u 177u 195u 12u "$(addToPath)"
  Pop $AddToPathCheckbox
  SetCtlColors $AddToPathCheckbox "" "FFFFFF"
  ${If} $KeepPathAlone <> 1
    ${NSD_Check} $AddToPathCheckbox
  ${EndIf}
FunctionEnd

Function AddToPathLeave
  ${NSD_GetState} $AddToPathCheckbox $0
  ${If} $0 == ${BST_CHECKED}
    StrCpy $KeepPathAlone 0
  ${Else}
    StrCpy $KeepPathAlone 1
  ${EndIf}
FunctionEnd

; Runs `config-digger installer <verb> "$INSTDIR"` and tells running
; programs that the environment changed.
!macro EditUserPath verb
  nsExec::ExecToLog '"$INSTDIR\${CLI_EXE}" installer ${verb} "$INSTDIR"'
  Pop $0
  ${If} $0 != 0
    DetailPrint "config-digger installer ${verb} failed: $0"
  ${EndIf}
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} $UpdateMode <> 1
    Call LoadPathChoice
    WriteRegDWORD HKCU "${PATH_CHOICE_KEY}" "${PATH_CHOICE_VALUE}" $KeepPathAlone
    ${If} $KeepPathAlone = 1
      !insertmacro EditUserPath remove-from-path
    ${Else}
      !insertmacro EditUserPath add-to-path
    ${EndIf}
  ${EndIf}
!macroend

; Tauri checks for a running app only after this hook: check first, so that
; cancelling its "Click OK to kill it" leaves the PATH and the data alone.
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
  ${If} $UpdateMode <> 1
    !insertmacro EditUserPath remove-from-path

    ${If} $DeleteAppDataCheckboxState = 1
      nsExec::ExecToStack '"$INSTDIR\${CLI_EXE}" installer parked'
      Pop $0
      Pop $1
      ${If} $0 != 0
        StrCpy $2 "$(deleteDataParkedUnknown)$\r$\n$\r$\n"
      ${ElseIf} $1 != ""
        StrCpy $2 "$(deleteDataParked)$\r$\n$\r$\n$1$\r$\n"
      ${Else}
        StrCpy $2 ""
      ${EndIf}
      ${IfNot} ${Cmd} `MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION|MB_DEFBUTTON2 "$(deleteDataWarning)$\r$\n$\r$\n$2$(deleteDataConfirm)" /SD IDCANCEL IDOK`
        StrCpy $DeleteAppDataCheckboxState 0
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "${DATA_LOCAL}"
    RmDir /r "${DATA_ROAMING}"
  ${EndIf}
!macroend
