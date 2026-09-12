; Rivulet bundles its own mpv, and mpv runs as a separate process.
;
; NSIS cannot overwrite a binary that is running, and a player process
; outlives an app that crashed or was killed from Task Manager — so an
; upgrade failed on "Error opening file for writing: ...\mpv\mpv.exe"
; with Abort / Retry / Ignore, and the only way past it was to hunt down
; the stray process by hand. Tauri's own template closes the app itself;
; it knows nothing about the children the app started.
;
; Matched on the executable's path, not just its name: someone else's mpv
; on the PATH is none of our business and must not be killed. The
; simplified `Where-Object Path -like` form is deliberate — it avoids
; `$_`, which NSIS would try to expand as one of its own variables.

!macro RIVULET_KILL_PLAYERS
  DetailPrint "Closing any leftover Rivulet player process..."
  nsExec::ExecToLog `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-Process -Name mpv,ffmpeg,yt-dlp -ErrorAction SilentlyContinue | Where-Object Path -like '*Rivulet*' | Stop-Process -Force -ErrorAction SilentlyContinue"`
  Pop $0
  ; Windows releases the file handle a moment after the process dies, and
  ; the copy that follows is immediate. Half a second costs nobody anything
  ; and is the difference between an upgrade and an Abort/Retry/Ignore box.
  Sleep 500
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro RIVULET_KILL_PLAYERS
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro RIVULET_KILL_PLAYERS
!macroend
