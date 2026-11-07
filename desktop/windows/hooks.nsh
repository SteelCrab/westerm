; NSIS hooks for the Windows installer (tauri.conf.json bundle.windows.nsis).
;
; The PTY host runs from westerm.exe in the install folder and outlives the
; app, so Windows will not let the installer replace or remove that file.
; `westerm stop` makes the host save every session and stop; the shells come
; back once as "restored" when the app opens again.

!include LogicLib.nsh

!macro WESTERM_STOP_HOST
  ${If} ${FileExists} "$INSTDIR\westerm.exe"
    ; Keep $0 for the installer: the exit code goes into it, then $0 comes back.
    Push $0
    nsExec::Exec '"$INSTDIR\westerm.exe" stop'
    Pop $0
    Pop $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro WESTERM_STOP_HOST
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro WESTERM_STOP_HOST
!macroend
