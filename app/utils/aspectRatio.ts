export function cycleAspect(current: 'contain' | 'cover' | 'fill'): 'contain' | 'cover' | 'fill' {
  return current === 'contain' ? 'cover' : current === 'cover' ? 'fill' : 'contain'
}

/** Drive fit / cover / stretch on native mpv, libVLC, or the htmlvideo shim. */
export function applyAspect(
  player: { ipc: (cmd: unknown[]) => unknown } | null | undefined,
  mode: 'contain' | 'cover' | 'fill',
) {
  if (!player)
    return
  // Android libVLC and the html `<video>` shim speak `video-scale`; mpv does not.
  player.ipc(['set_property', 'video-scale', mode])
  // mpv: letterbox, crop-to-fill, or stretch.
  switch (mode) {
    case 'contain':
      player.ipc(['set_property', 'keepaspect', true])
      player.ipc(['set_property', 'panscan', 0])
      break
    case 'cover':
      player.ipc(['set_property', 'keepaspect', true])
      player.ipc(['set_property', 'panscan', 1])
      break
    case 'fill':
      player.ipc(['set_property', 'keepaspect', false])
      player.ipc(['set_property', 'panscan', 0])
      break
  }
}
