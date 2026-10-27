; config-digger's additions to Tauri's NSIS installer, which includes this
; file before its own pages (`bundle.windows.nsis.installerHooks`).
;
; - The install folder is `%LOCALAPPDATA%\Programs\config-digger`, not
;   Tauri's `%LOCALAPPDATA%\config-digger`, which is a data folder. The
;   installer never installs in a data folder, nor inside or above one, so
;   an uninstall cannot take the data with the app, and the PATH entry is
;   no data folder.
; - The welcome page carries a checkbox, checked by default, that puts the
;   install folder on the user PATH (HKCU). The uninstaller takes it off.
;   An unticked box is remembered for the next install, silent ones included.
;   `$PROFILE` is never touched. The PATH is edited by the console exe,
;   `config-digger installer add-to-path|remove-from-path`, because NSIS
;   strings stop at 1024 characters and would cut a long PATH. When the
;   edit fails, a GUI run warns and names the folder to add or remove by
;   hand; a silent one only says so in the install details.
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

; The install folder: a folder of its own, which `cargo xtask installer`
; checks is no data folder, nor a folder inside or above one.
!define INSTALL_FOLDER "$LOCALAPPDATA\Programs\config-digger"

; Spells the folder on top of the stack as a full path, with `..` resolved
; and slashes turned into backslashes, whether it exists or not, and with
; one trailing backslash, so that `C:\a\` is not taken for being inside
; `C:\ab\`. NSIS's own GetFullPathName fails on a folder that doesn't exist.
; The backslash goes on first: `C:` alone, as NSIS spells `C:\` in $INSTDIR,
; would be drive C's current folder.
Function SpelledFolder
  Exch $0
  Push $1
  Push $2
  StrCpy $0 "$0\"
  System::Call 'kernel32::GetFullPathNameW(w r0, i ${NSIS_MAX_STRLEN}, w .r1, p 0) i .r2'
  ${If} $2 <> 0
  ${AndIf} $2 < ${NSIS_MAX_STRLEN}
    StrCpy $0 $1
  ${EndIf}
  Pop $2
  ${Do}
    StrCpy $1 $0 "" -1
    ${If} $1 != "\"
      ${ExitDo}
    ${EndIf}
    StrCpy $0 $0 -1
  ${Loop}
  StrCpy $0 "$0\"
  Pop $1
  Exch $0
FunctionEnd

; Within IsDataFolder: sets $3 to 1 when the folder in $0, with one
; trailing backslash, is `data`, inside it or above it.
!macro CompareWithDataFolder data
  Push "${data}"
  Call SpelledFolder
  Pop $1
  ; Whichever of the two is shorter starts the other.
  StrLen $2 $1
  StrCpy $2 $0 $2
  ${If} $2 == $1
    StrCpy $3 1
  ${EndIf}
  StrLen $2 $0
  StrCpy $2 $1 $2
  ${If} $2 == $0
    StrCpy $3 1
  ${EndIf}
!macroend

; Replaces the folder on top of the stack with 1 when it is one of the data
; folders, or a folder inside or above one, and with 0 otherwise. NSIS
; compares strings without case.
Function IsDataFolder
  Exch $0
  Push $1
  Push $2
  Push $3
  Push $0
  Call SpelledFolder
  Pop $0
  StrCpy $3 0
  !insertmacro CompareWithDataFolder "${DATA_LOCAL}"
  !insertmacro CompareWithDataFolder "${DATA_ROAMING}"
  StrCpy $0 $3
  Pop $3
  Pop $2
  Pop $1
  Exch $0
FunctionEnd

; Moves $INSTDIR to the install folder when it is a data folder, or a folder
; inside or above one: Tauri's default for `currentUser`,
; `$LOCALAPPDATA\${PRODUCTNAME}`, which `cargo xtask installer` checks is a
; data folder, a previous install's folder or a `/D=` one.
; With the data out of the install folder, an uninstall keeps it however
; Tauri removes that folder.
Function UseOwnInstallFolder
  Push $0
  Push $INSTDIR
  Call IsDataFolder
  Pop $0
  ${If} $0 = 1
    StrCpy $INSTDIR "${INSTALL_FOLDER}"
  ${EndIf}
  Pop $0
FunctionEnd

; Before the first page, so the directory page offers the install folder:
; after Tauri's `.onInit` has set its default or the previous install's.
!define MUI_CUSTOMFUNCTION_GUIINIT UseOwnInstallFolder

; The directory page's Next button stays disabled on a data folder, and the
; page says so. Tauri's directory page is the first to read this.
!define MUI_DIRECTORYPAGE_TEXT_TOP "$(installFolderText)"

Function .onVerifyInstDir
  Push $0
  Push $INSTDIR
  Call IsDataFolder
  Pop $0
  ${If} $0 = 1
    Pop $0
    Abort
  ${EndIf}
  Pop $0
FunctionEnd

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
; programs that the environment changed. When the edit fails, or the CLI
; doesn't start, the install details say so, and a GUI run shows
; `warning`, which says what to do by hand, with the folder. A silent run
; skips it, and its exit code stays as it was.
!macro EditUserPath verb warning
  nsExec::ExecToLog '"$INSTDIR\${CLI_EXE}" installer ${verb} "$INSTDIR"'
  Pop $0
  ${If} $0 != 0
    DetailPrint "config-digger installer ${verb} failed: $0"
    MessageBox MB_OK|MB_ICONEXCLAMATION "${warning}$\r$\n$\r$\n$INSTDIR" /SD IDOK
  ${EndIf}
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

; A silent install has no GUI to have moved $INSTDIR. Tauri's `SetOutPath`
; has run by now: on a data folder, it creates at most that empty folder.
!macro NSIS_HOOK_PREINSTALL
  Call UseOwnInstallFolder
  SetOutPath $INSTDIR
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} $UpdateMode <> 1
    Call LoadPathChoice
    WriteRegDWORD HKCU "${PATH_CHOICE_KEY}" "${PATH_CHOICE_VALUE}" $KeepPathAlone
    ${If} $KeepPathAlone = 1
      !insertmacro EditUserPath remove-from-path "$(pathNotRemoved)"
    ${Else}
      !insertmacro EditUserPath add-to-path "$(pathNotAdded)"
    ${EndIf}
  ${EndIf}
!macroend

; Tauri checks for a running app only after this hook: check first, so that
; cancelling its "Click OK to kill it" leaves the PATH and the data alone.
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
  ${If} $UpdateMode <> 1
    !insertmacro EditUserPath remove-from-path "$(pathNotRemoved)"

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
