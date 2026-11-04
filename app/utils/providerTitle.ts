import { tierLabel, tierOfText } from './quality'

/**
 * IPTV VOD names are a panel row, not a film title: `EN: Dune (2021)`,
 * `PK | Film [4K]`. TMDB has never heard of the prefix, so a search on
 * the raw string returns nothing and the detail page keeps "No overview."
 */

const PREFIX = /^(?:[A-Z]{2}\s*[:|.–—-]\s*|\[[A-Z]{2}\]\s*)/i
const YEAR = /\(((?:19|20)\d{2})\)/

export function hasProviderPrefix(title: string): boolean {
  return PREFIX.test(title.trim())
}

export function stripProviderPrefix(title: string): string {
  return title.replace(PREFIX, '').trim()
}

/** `| 4K`, `[FHD]` at the end: a panel's quality stamp, not part of the title. */
const QUALITY_TAIL = /\s*(?:\||\[)\s*(4K|UHD|FHD|HD|SD|HEVC|1080p|720p)\s*(?:\]\s*)?$/i
/** `- 2026`, `| 2026` at the end. Only after a separator, so `1917` and `2012` stay titles. */
const YEAR_TAIL = /\s+[-–—|]\s*((?:19|20)\d{2})\s*$/

/**
 * Title, year and quality for the detail page and for a TMDB query. Panels
 * write `Cold Storage - 2026` and `Project Hail Mary - 2026 | 4K`; TMDB
 * matches neither with the tail on, and an unmatched title has no logo, no
 * synopsis and no cast.
 */
export function vodDisplayName(title: string): { name: string, year: string, quality: string } {
  let rest = stripProviderPrefix(title)
  const quality = rest.match(QUALITY_TAIL)?.[1]?.toUpperCase() ?? ''
  rest = rest.replace(QUALITY_TAIL, '')
  const year = rest.match(YEAR)?.[1] ?? rest.match(YEAR_TAIL)?.[1] ?? ''
  const name = rest.replace(YEAR, '').replace(YEAR_TAIL, '').replace(/\s+/g, ' ').trim()
  return { name: name || title.trim(), year, quality }
}

/**
 * Every listing of one title at a different quality — panels list `Dune | 4K`
 * and `Dune | FHD` as two films, and those are the only qualities an Xtream
 * VOD really has (each entry is one file, with no variants inside it). Highest
 * first, one per resolution, and empty unless there are at least two to pick
 * between. `label` is `''` for a listing with no stamp at all.
 */
export function vodVersions<T extends { id: string, name: string }>(items: readonly T[], title: string): { item: T, label: string }[] {
  const me = vodDisplayName(title)
  const want = me.name.toLowerCase()
  if (!want)
    return []
  const byTier = new Map<string, { item: T, label: string, height: number }>()
  for (const item of items) {
    // Cheap test first: a panel can hold fifty thousand films.
    if (!item.name.toLowerCase().includes(want))
      continue
    const it = vodDisplayName(item.name)
    if (it.name.toLowerCase() !== want || (me.year && it.year && me.year !== it.year))
      continue
    const tier = tierOfText(it.quality) || tierOfText(item.name)
    const key = tier || it.quality
    if (!byTier.has(key))
      byTier.set(key, { item, label: tierLabel(tier) || it.quality, height: Number(tier) || 0 })
  }
  return byTier.size > 1
    ? [...byTier.values()].sort((a, b) => b.height - a.height).map(({ item, label }) => ({ item, label }))
    : []
}
