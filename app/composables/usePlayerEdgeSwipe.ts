import { onBeforeUnmount, ref } from 'vue'
import { clearScreenBrightness, isAndroid, isTv, mediaVolume, screenBrightness, setMediaVolume, setScreenBrightness } from '~/utils/platform'
import { fmtHudTime } from '~/utils/playbackError'
import { clampLevel, edgeAdjust, edgeDelta, isHorizontalSwipe, isVerticalSwipe, seekSeconds } from '~/utils/playerSwipe'

export interface PlayerEdgeHud {
  kind: 'volume' | 'brightness' | 'seek'
  level: number
  caption?: string
}

/**
 * Picture gestures shared by the film player and the live overlay.
 *
 * Vertical left = volume, vertical right = brightness, horizontal = seek
 * (when `seek` is passed and the title has a duration). Android WebView
 * reports a finger as a mouse and often drops `pointermove` on a transparent
 * catcher, so the drag is tracked on `window` with touch events — the same
 * path the nav drawer already uses on this WebView.
 */
export function usePlayerEdgeSwipe(opts: {
  enabled: () => boolean
  volume: () => number
  setVolume: (n: number) => void
  seek?: (t: number) => void
  position?: () => number
  duration?: () => number
}) {
  const hud = ref<PlayerEdgeHud | null>(null)
  const swiping = ref(false)
  let drag: {
    id: number
    x: number
    y: number
    w: number
    h: number
    left: number
    kind: 'volume' | 'brightness' | 'seek' | null
    origin: number
  } | null = null
  let hudTimer: ReturnType<typeof setTimeout> | null = null
  let touchedBrightness = false
  let listening = false
  let ateTap = false

  function hideHud() {
    if (hudTimer)
      clearTimeout(hudTimer)
    hudTimer = setTimeout(() => {
      hud.value = null
    }, 700)
  }

  function applyAt(clientX: number, clientY: number) {
    if (!drag?.kind)
      return
    if (drag.kind === 'seek') {
      const dur = opts.duration?.() ?? 0
      const delta = seekSeconds(clientX - drag.x, drag.w, dur)
      const t = Math.max(0, Math.min(dur, drag.origin + delta))
      opts.seek?.(t)
      hud.value = {
        kind: 'seek',
        level: dur ? Math.round((t / dur) * 100) : 0,
        caption: `${delta >= 0 ? '+' : ''}${Math.round(delta)}s · ${fmtHudTime(t)}`,
      }
      return
    }
    const n = clampLevel(drag.origin + edgeDelta(clientY - drag.y, drag.h))
    if (drag.kind === 'brightness') {
      if (!setScreenBrightness(n))
        return
      touchedBrightness = true
    }
    else if (!setMediaVolume(n)) {
      opts.setVolume(n)
    }
    hud.value = { kind: drag.kind, level: n }
  }

  function bindWin() {
    if (listening)
      return
    listening = true
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUpWin)
    window.addEventListener('pointercancel', onUpWin)
    window.addEventListener('touchmove', onTouchMove, { passive: false })
    window.addEventListener('touchend', onTouchEnd)
    window.addEventListener('touchcancel', onTouchEnd)
  }

  function unbindWin() {
    if (!listening)
      return
    listening = false
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUpWin)
    window.removeEventListener('pointercancel', onUpWin)
    window.removeEventListener('touchmove', onTouchMove)
    window.removeEventListener('touchend', onTouchEnd)
    window.removeEventListener('touchcancel', onTouchEnd)
  }

  function pickKind(dx: number, dy: number, startX: number, width: number) {
    if (isVerticalSwipe(dx, dy))
      return edgeAdjust(startX, width)
    const dur = opts.duration?.() ?? 0
    if (opts.seek && dur > 0 && isHorizontalSwipe(dx, dy))
      return 'seek' as const
    return null
  }

  function originFor(kind: 'volume' | 'brightness' | 'seek') {
    if (kind === 'volume')
      return mediaVolume() ?? opts.volume()
    if (kind === 'brightness')
      return screenBrightness() ?? 70
    return opts.position?.() ?? 0
  }

  function begin(x: number, y: number, id: number, box: DOMRect) {
    if (drag || !opts.enabled() || isTv() === true)
      return
    if (box.width <= 0)
      return
    drag = { id, x, y, w: box.width, h: box.height, left: box.left, kind: null, origin: 0 }
    swiping.value = false
    ateTap = false
    bindWin()
  }

  function onDown(e: PointerEvent) {
    const el = e.currentTarget as HTMLElement | null
    const box = el?.getBoundingClientRect()
    if (!box)
      return
    begin(e.clientX, e.clientY, e.pointerId, box)
    if (!isAndroid() && el) {
      try {
        el.setPointerCapture(e.pointerId)
      }
      catch { /* capture is optional */ }
    }
  }

  function onTouchStart(e: TouchEvent) {
    const t = e.touches[0]
    const el = e.currentTarget as HTMLElement | null
    if (!t || !el)
      return
    begin(t.clientX, t.clientY, -1, el.getBoundingClientRect())
  }

  function moveAt(clientX: number, clientY: number) {
    if (!drag)
      return
    if (!drag.kind) {
      const kind = pickKind(clientX - drag.x, clientY - drag.y, drag.x - drag.left, drag.w)
      if (!kind)
        return
      if (kind === 'brightness' && screenBrightness() === null)
        return
      drag.kind = kind
      drag.origin = originFor(kind)
      swiping.value = true
      if (hudTimer)
        clearTimeout(hudTimer)
    }
    applyAt(clientX, clientY)
  }

  function onMove(e: PointerEvent) {
    if (!drag || (drag.id !== -1 && e.pointerId !== drag.id))
      return
    moveAt(e.clientX, e.clientY)
  }

  function onTouchMove(e: TouchEvent) {
    if (!drag)
      return
    const t = e.touches[0]
    if (!t)
      return
    if (swiping.value)
      e.preventDefault()
    moveAt(t.clientX, t.clientY)
  }

  function end(did: boolean): boolean {
    unbindWin()
    drag = null
    swiping.value = false
    if (did) {
      ateTap = true
      hideHud()
    }
    return did
  }

  function onUp(e?: PointerEvent): boolean {
    if (!drag) {
      const did = ateTap
      ateTap = false
      return did
    }
    if (e && drag.id !== -1 && e.pointerId !== drag.id)
      return false
    const did = end(swiping.value)
    ateTap = false
    return did
  }

  function onUpWin(e: PointerEvent) {
    onUp(e)
  }

  function onTouchEnd() {
    if (!drag)
      return
    end(swiping.value)
  }

  onBeforeUnmount(() => {
    unbindWin()
    if (hudTimer)
      clearTimeout(hudTimer)
    if (touchedBrightness)
      clearScreenBrightness()
  })

  return { hud, swiping, onDown, onMove, onUp, onTouchStart }
}
