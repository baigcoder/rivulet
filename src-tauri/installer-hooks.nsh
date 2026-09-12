; Rivulet bundles its own mpv, and mpv runs as a separate process.
;
; NSIS cannot overwrite a binary that is running, and a player process
; outlives an app that crashed or was killed from Task Manager — so an
; upgrade failed on "Error opening file for writing: ...\mpv\mpv.exe"
; with Abort / Retry / Ignore, and the only way past it was to find the
; stray process by hand. Tauri's own template closes the app itself; it
; knows nothing about the children the app started.
;
; Matched on the executable's path, not just its name: someone else's mpv
; on the PATH is none of our business and must not be killed.

!macro RIVULET_KILL_PLAYERS
  DetailPrint "Closing any leftover Rivulet player process..."
  nsExec::ExecToLog `powershell -NoProfile -NonInteractive -Command "Get-Process -Name mpv,ffmpeg -ErrorAction SilentlyContinue | Where-Object Path -like '*Rivulet*' | Stop-Process -Force -ErrorAction SilentlyContinue"`
  Pop $0
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro RIVULET_KILL_PLAYERS
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro RIVULET_KILL_PLAYERS
!macroend
