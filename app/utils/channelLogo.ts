/**
 * IPTV providers often ship one generic logo for whole categories — a
 * white "24/7" square repeated thousands of times. Treat those like
 * missing art so the tile falls back to initials on a distinct colour.
 */

const PLACEHOLDER_URL = /24[\s\-_./x]*7|default|placeholder|no[_-]?logo|dummy|generic|templ|missing|unknown/i

export function isPlaceholderLogoUrl(url: string | null | undefined): boolean {
  if (!url)
    return true
  const u = url.trim()
  if (!u || u === 'null' || u === 'undefined' || u.length < 8)
    return true
  return PLACEHOLDER_URL.test(u)
}

/** Loaded image is too small to be a real channel mark. */
export function isTinyLogo(img: HTMLImageElement): boolean {
  return img.naturalWidth > 0 && img.naturalHeight > 0
    && img.naturalWidth < 56 && img.naturalHeight < 56
}

/** Stable accent for initials tiles — distinct per channel, dark enough for white text. */
export function channelTileStyle(seed: string): { background: string } {
  let hash = 0
  for (let i = 0; i < seed.length; i++)
    hash = (hash * 31 + seed.charCodeAt(i)) >>> 0
  const hue = hash % 360
  return {
    background: `linear-gradient(145deg, hsl(${hue} 42% 28%) 0%, hsl(${hue} 36% 16%) 100%)`,
  }
}
