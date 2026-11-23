; The default per-user install directory (%LOCALAPPDATA%\Echoless) is also the
; Echoless data root. The built-in "Delete the application data" option only
; removes %LOCALAPPDATA%\app.echoless.desktop, so remove the data directories
; under $INSTDIR here. Updates keep all data.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    RMDir /r "$INSTDIR\logs"
    RMDir /r "$INSTDIR\diagnostics"
    RMDir /r "$INSTDIR\localvqe"
    RMDir /r "$INSTDIR\nvafx"
    RMDir "$INSTDIR"
  ${EndIf}
!macroend
