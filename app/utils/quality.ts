/**
 * Resolution, read from whatever a source wrote: a release name, an addon's
 * "MediaFusion\n4K" label, a file name, a provider's "FHD" stamp.
 *
 * A tier is the vertical line count as a string (`'2160'`, `'1080'`, …) and
 * `''` for a name that says nothing — never guessed, so a menu built from tiers
 * can only offer what a source actually carries.
 */
export type Tier = '2160' | '1440' | '1080' | '720' | '480' | ''

/** Highest first — the order every quality menu lists them in. */
export const TIERS: Exclude<Tier, ''>[] = ['2160', '1440', '1080', '720', '480']

const LABEL: Record<Exclude<Tier, ''>, string> = {
  2160: '4K',
  1440: '2K',
  1080: '1080p',
  720: '720p',
  480: '480p',
}

export function tierLabel(tier: Tier) {
  return tier ? LABEL[tier] : ''
}

/**
 * Strongest evidence first. Separators are spaces by the time these run, since
 * `\b` doesn't fire between `_` and a digit ("Film_1080p_x264").
 */
const PATTERNS: [Exclude<Tier, ''>, RegExp][] = [
  ['2160', /\b(?:4320p|8k|2160[pi]?|4k|uhd)/],
  ['1440', /\b(?:1440[pi]|2k|qhd)\b/],
  ['1080', /\b(?:1080[pi]|fhd|full\s?hd|bd1080)\b/],
  ['720', /\b(?:720p|hd\s?720)\b/],
  ['480', /\b(?:576[pi]|540p|480[pi]|360p|240p|sd|sdtv|dvd\s?rip|dvd(?:scr)?|xvid|divx)\b/],
]

/** A bare number only counts boxed in — "[1080]" — never loose, where it is a year or a runtime. */
const BOXED = /[[(](2160|1440|1080|720|576|480)[\])]/

/** "HD" alone is a provider stamp for 720p; never the start of HDR, HDTV or HDRip. */
const LONE_HD = /\bhd\b(?![\s-](?:r|tv|cam|ts|10|light))/

/** "1920x800" — read by width, since a scope film crops the height and keeps the width. */
const FRAME = /\b(\d{3,4})x(\d{3,4})\b/

function tierOfWidth(width: number): Tier {
  if (width >= 3200)
    return '2160'
  if (width >= 2200)
    return '1440'
  if (width >= 1700)
    return '1080'
  if (width >= 1100)
    return '720'
  return width >= 320 ? '480' : ''
}

export function tierOfText(text: string | null | undefined): Tier {
  const s = (text ?? '').toLowerCase().replace(/[._]+/g, ' ')
  if (!s.trim())
    return ''
  const frame = s.match(FRAME)
  if (frame)
    return tierOfWidth(Number(frame[1]))
  for (const [tier, re] of PATTERNS) {
    if (re.test(s))
      return tier
  }
  const boxed = s.match(BOXED)?.[1]
  if (boxed)
    return boxed === '576' ? '480' : boxed as Tier
  return LONE_HD.test(s) ? '720' : ''
}

/** A release's tier: its quality label first, then its name, then the file it plays. */
export function tierOf(r: { quality?: string, name?: string, file?: string | null }): Tier {
  return tierOfText(r.quality) || tierOfText(r.name) || tierOfText(r.file)
}

/** Every tier present in a list, highest first. */
export function tiersIn<T extends { quality?: string, name?: string, file?: string | null }>(list: T[]): Exclude<Tier, ''>[] {
  const have = new Set(list.map(tierOf))
  return TIERS.filter(t => have.has(t))
}

/** A height in pixels (mpv's `height`, an HLS `RESOLUTION`) to the tier it plays as. */
export function tierOfHeight(height: number): Tier {
  if (!(height > 0))
    return ''
  // Cropped widescreen: a 1920x800 film is 1080p however few lines it has.
  if (height >= 1600)
    return '2160'
  if (height >= 1200)
    return '1440'
  if (height >= 780)
    return '1080'
  if (height >= 600)
    return '720'
  return '480'
}

export type ReleaseSort = 'quality-desc' | 'quality-asc' | 'size-desc' | 'size-asc' | 'seeders'

interface Sortable { quality?: string, name?: string, file?: string | null, bytes: number, seeders: number, url?: string }

const height = (r: Sortable) => Number(tierOf(r)) || 0

/**
 * The release list in the order the viewer asked for. Ties fall to seeders, then
 * size, so the order is stable from one search to the next. A name with no
 * resolution in it goes last either way — "unknown" is not the lowest quality.
 */
export function sortReleases<T extends Sortable>(list: T[], by: ReleaseSort): T[] {
  const known = (r: T) => Number(!height(r))
  const tie = (a: T, b: T) => b.seeders - a.seeders || b.bytes - a.bytes
  const order: Record<ReleaseSort, (a: T, b: T) => number> = {
    'quality-desc': (a, b) => known(a) - known(b) || height(b) - height(a) || tie(a, b),
    'quality-asc': (a, b) => known(a) - known(b) || height(a) - height(b) || tie(a, b),
    // A size the source never reported sorts last, not first.
    'size-desc': (a, b) => Number(!a.bytes) - Number(!b.bytes) || b.bytes - a.bytes || tie(a, b),
    'size-asc': (a, b) => Number(!a.bytes) - Number(!b.bytes) || a.bytes - b.bytes || tie(a, b),
    // A direct link has no swarm; it sorts after every torrent rather than as zero.
    'seeders': (a, b) => Number(!!a.url && !a.seeders) - Number(!!b.url && !b.seeders) || tie(a, b),
  }
  return [...list].sort(order[by])
}

/**
 * Narrow to one tier when the list has it. Empty tier, or none at that tier,
 * hands the list back untouched — asking for 480p never leaves nothing to play.
 */
export function atTier<T extends { quality?: string, name?: string, file?: string | null }>(list: T[], tier: Tier | string | undefined | null): T[] {
  if (!tier)
    return list
  const hit = list.filter(r => tierOf(r) === tier)
  return hit.length ? hit : list
}

/** `?q=` as a tier, or `''` for anything that isn't one. */
export function asTier(value: unknown): Tier {
  const s = String(value ?? '')
  return (TIERS as string[]).includes(s) ? s as Tier : ''
}
