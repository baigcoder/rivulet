; Rivulet bundles its own mpv, and mpv runs as a separate process.
;
; NSIS cannot overwrite a binary that is running, so an upgrade failed on
; "Error opening file for writing: ...\mpv\mpv.exe" with Abort/Retry/Ignore.
; Tauri's own template closes the app; it knows nothing about the children
; the app started.
;
; Killing those children was the first attempt and it was not enough — the
; installer log showed the kill running and the very next extract still
; refusing. Either the app was alive and started a new player, or the handle
; outlived the process by longer than we waited. Both are races, and neither
; is worth trying to win.
;
; So the kill is now only the tidy path. The one that actually holds is
; RIVULET_FREE_FILE: Windows refuses to *overwrite* a running binary but is
; perfectly willing to *rename* one, so the old file is moved aside and the
; installer writes into a name nothing holds. The stale copy goes on the
; reboot queue rather than being left as clutter.

!macro RIVULET_FREE_FILE FILE
  Delete "${FILE}"
  ; Still there means something holds it. Rename works anyway; the new file
  ; then installs over a path with no handle on it.
  IfFileExists "${FILE}" 0 +3
  Rename "${FILE}" "${FILE}.old"
  Delete /REBOOTOK "${FILE}.old"
!macroend

!macro RIVULET_MAKE_WAY
  DetailPrint "Closing Rivulet and any player it started..."
  ; The app first, so nothing is left running that would start a new player
  ; behind us. Matched on path, never on name alone: someone else's mpv is
  ; none of our business. No `$` anywhere in the script — NSIS would try to
  ; expand it as one of its own variables.
  nsExec::ExecToLog `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-Process -Name rivulet,mpv,ffmpeg,yt-dlp -ErrorAction SilentlyContinue | Where-Object Path -like '*Rivulet*' | Stop-Process -Force -ErrorAction SilentlyContinue"`
  Pop $0
  ; Windows releases the handle a moment after the process dies.
  Sleep 500

  ; Whatever survived that, take the name away from it.
  !insertmacro RIVULET_FREE_FILE "$INSTDIR\mpv\mpv.exe"
  !insertmacro RIVULET_FREE_FILE "$INSTDIR\mpv\ffmpeg.exe"
  !insertmacro RIVULET_FREE_FILE "$INSTDIR\ytdlp\yt-dlp.exe"
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro RIVULET_MAKE_WAY
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Closing Rivulet and any player it started..."
  nsExec::ExecToLog `powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-Process -Name rivulet,mpv,ffmpeg,yt-dlp -ErrorAction SilentlyContinue | Where-Object Path -like '*Rivulet*' | Stop-Process -Force -ErrorAction SilentlyContinue"`
  Pop $0
  Sleep 500
!macroend
