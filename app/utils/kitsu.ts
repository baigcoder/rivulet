/**
 * Anime by Kitsu id, for the addons that look anime up that way.
 *
 * Plenty of anime have no IMDb id at all, and where one exists its seasons
 * rarely line up with how a show was released — so a source that indexes
 * anime by Kitsu answered "nothing" for titles it had in full. Kitsu gives
 * each season its own entry and numbers episodes within it, which is exactly
 * the shape a `kitsu:<entry>:<episode>` request wants.
 *
 * Kitsu's public API needs no key and allows cross-origin reads; a title is
 * looked up only when it is anime and an addon is about to be asked for it.
 */

const API = 'https://kitsu.app/api/edge/anime'

/** One search result, reduced to what choosing between them needs. */
export interface KitsuEntry {
  id: string
  titles: string[]
  subtype: string
  year: number
}

/** "Season 2", "2nd Season", "Part 2"… — the season an entry says it is, 1 when it says none. */
export function kitsuSeason(titles: string[]): number {
  for (const t of titles) {
    const m = t.match(/\bseason\s*(\d+)\b|\b(\d+)(?:st|nd|rd|th)\s+season\b/i)
    if (m)
      return Number(m[1] ?? m[2])
  }
  return 1
}

/**
 * The entry Play means: a film for a film, a series for a series, and among
 * those the season asked for. A year, when known, settles season 1 between a
 * show and its remake. Null when nothing fits — better no answer than the
 * wrong show.
 */
export function pickKitsuEntry(entries: KitsuEntry[], want: { movie: boolean, season: number, year?: number }): string | null {
  const kind = entries.filter(e => want.movie ? e.subtype === 'movie' : ['TV', 'ONA', 'OVA'].includes(e.subtype))
  const season = want.movie ? kind : kind.filter(e => kitsuSeason(e.titles) === want.season)
  if (!season.length)
    return null
  const byYear = want.year && (want.movie || want.season === 1)
    ? season.filter(e => Math.abs(e.year - want.year!) <= 1)
    : season
  const pool = byYear.length ? byYear : season
  return [...pool].sort((a, b) => a.year - b.year)[0]!.id
}

/** `kitsu:<id>` for an anime, or null when Kitsu has nothing that clearly fits. */
export async function kitsuIdFor(title: string, options: { movie: boolean, season: number, year?: number }): Promise<string | null> {
  if (!title)
    return null
  const query = options.movie || options.season <= 1 ? title : `${title} season ${options.season}`
  const url = `${API}?filter%5Btext%5D=${encodeURIComponent(query)}&page%5Blimit%5D=10&fields%5Banime%5D=canonicalTitle,titles,subtype,startDate`
  try {
    const res = await fetch(url, { signal: AbortSignal.timeout(8000) })
    if (!res.ok)
      return null
    const data = await res.json() as { data?: { id: string, attributes: { canonicalTitle?: string, titles?: Record<string, string | null>, subtype?: string, startDate?: string } }[] }
    const entries: KitsuEntry[] = (data.data ?? []).map(a => ({
      id: a.id,
      titles: [a.attributes.canonicalTitle ?? '', ...Object.values(a.attributes.titles ?? {}).filter((t): t is string => !!t)],
      subtype: a.attributes.subtype ?? '',
      year: Number(a.attributes.startDate?.slice(0, 4)) || 0,
    }))
    const id = pickKitsuEntry(entries, options)
    return id ? `kitsu:${id}` : null
  }
  catch {
    return null
  }
}
