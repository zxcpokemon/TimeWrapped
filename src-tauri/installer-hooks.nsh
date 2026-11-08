!macro NSIS_HOOK_PREUNINSTALL
  ; Ask the native collector to commit its buffer and exit before removal.
  nsExec::ExecToLog '"$INSTDIR\TimeWrapped.exe" --quit'
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "TimeWrapped"
  Sleep 800
!macroend
