import type { Media } from '~/utils/tmdb'
import { backdropUrl, posterUrl } from '~/utils/tmdb'

const warmed = new Set<string>()

function warm(url: string | null | undefined) {
  if (!url || warmed.has(url))
    return
  warmed.add(url)
  const img = new Image()
  img.src = url
}

/** Decode one image URL — provider posters are already absolute. */
export function warmUrl(url: string | null | undefined) {
  warm(url ?? undefined)
}

/** Decode hero/window art after the click — w780 only; the window can upscale. */
export function warmArt(media: Pick<Media, 'poster' | 'backdrop'>) {
  warm(posterUrl(media.poster, 'w780') ?? undefined)
  warm(backdropUrl(media.backdrop, 'w780') ?? undefined)
}
