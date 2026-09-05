import type { Media, MediaType } from '~/utils/tmdb'

/** Title routes keep the selected theme — Home is keepalive and would otherwise leave poster art behind the page. */
export function isTitlePath(path: string) {
  return /^\/(?:movie|tv)(?:\/|$)/.test(path)
    || path.includes('/collection/')
    || /\/live-tv\/premium\/(?:movie|series)\//.test(path)
}

/** Plain memory — no Pinia on the click path that opens a title. */
let armed: Media | null = null

export function armDetail(media: Media) {
  armed = media
  snapMedia(media)
  prefetchMediaDetail(media)
  // Hero backdrop is w780; the card only decoded the poster.
  warmUrl(backdropUrl(media.backdrop, 'w780'))
}

export function armDetailPress(event: PointerEvent, media: Media) {
  if (event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey)
    armDetail(media)
}

/**
 * Keyboard activation never saw pointerdown — arm the splash now.
 * Navigation stays on NuxtLink; blocking the link and routing in a timer
 * left users stuck on the opening overlay with no route change.
 */
export function openDetail(event: MouseEvent, media: Media) {
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
    return
  if (event.detail === 0)
    armDetail(media)
}

export function takeArmed(type: MediaType, id: string | number): Media | null {
  if (!armed || armed.type !== type || String(armed.id) !== String(id))
    return null
  const m = armed
  armed = null
  return m
}
