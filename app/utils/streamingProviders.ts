/**
 * Which watch providers the Home strip shows, in what order, and how many.
 *
 * TMDB's regional list is not a list of services. In the US it is 334 entries,
 * and most are the same service sold through someone else ("HBO Max Amazon
 * Channel", "AMC Plus Apple TV channel"), a cut-down plan of one ("Netflix
 * Kids", "… with Ads"), a tier ("Paramount Plus Premium" and "Essential"), or a
 * shop that rents titles rather than a catalogue to browse. Worse, TMDB ranks
 * the resold copy above the real one — HBO Max's Amazon channel sits at 11 and
 * HBO Max itself at 152 — so taking the first entry per name put Prime Video's
 * logo on the HBO Max card. This keeps one card per service, wearing that
 * service's own name and logo.
 *
 * Pure, so `check:library` runs it against TMDB's real names.
 */

export interface WatchProvider {
  provider_id: number
  provider_name: string
  logo_path: string | null
  display_priority?: number
  /** Per country; lower is more prominent. */
  display_priorities?: Record<string, number>
}

/** Sold through another store: the service's own entry is the card. */
const RESOLD = /\b(?:amazon channels?|apple tv channel|roku premium channel)\b/i

/** A cut-down plan of a service that is listed in full elsewhere. */
const PLAN = /\b(?:with ads|kids)\b/i

/** Places to buy or rent a title — nothing to browse as a service. */
const STORE = /^(?:amazon video|apple tv store|google play movies|fandango at home(?: free)?|microsoft store|vudu|justwatch tv|spectrum on demand|directv|youtube|youtube free)$/i

/**
 * The household names lead, in this order; everything else follows TMDB's own
 * prominence for the region. An ordering choice, not a recommendation of
 * anywhere to watch anything — what plays is decided by the user's sources.
 */
const PINNED: string[][] = [
  ['netflix'],
  ['amazon prime video', 'prime video'],
  ['apple tv', 'apple tv plus'],
  ['hbo max', 'max'],
  ['disney plus'],
  ['hulu'],
  ['paramount plus', 'paramount'],
  ['peacock'],
  ['crunchyroll'],
  ['amc plus'],
]

/** "Paramount+" and "Paramount Plus" compare equal. */
function normalize(name: string): string {
  return name.toLowerCase().replace(/\+/g, ' plus ').replace(/[^a-z0-9]+/g, ' ').replace(/\s+/g, ' ').trim()
}

/** One key per service: its tiers ("Premium", "Essential", "Premium Plus") fold away. */
function brandOf(name: string): string {
  let key = normalize(name)
  for (;;) {
    const next = key.replace(/ (?:premium plus|premium|essential|basic|standard)$/, '')
    if (next === key)
      return key
    key = next
  }
}

export function streamingProviders(list: readonly WatchProvider[], region: string): WatchProvider[] {
  const prominence = (p: WatchProvider) =>
    p.display_priorities?.[region] ?? p.display_priority ?? Number.MAX_SAFE_INTEGER

  const brands = new Map<string, { best: WatchProvider, prominence: number }>()
  for (const p of list) {
    const name = p.provider_name?.trim()
    if (!name || RESOLD.test(name) || PLAN.test(name) || STORE.test(name))
      continue
    const key = brandOf(name)
    const seen = brands.get(key)
    if (!seen) {
      brands.set(key, { best: p, prominence: prominence(p) })
      continue
    }
    // The tier the region ranks highest decides where the card sits; the
    // plainest name ("Peacock Premium" over "Peacock Premium Plus") is what
    // it is called and whose logo it shows.
    seen.prominence = Math.min(seen.prominence, prominence(p))
    if (normalize(name).length < normalize(seen.best.provider_name).length)
      seen.best = p
  }

  const pinnedAt = (key: string) => {
    const at = PINNED.findIndex(aliases => aliases.includes(key))
    return at === -1 ? PINNED.length : at
  }
  return [...brands.entries()]
    .sort(([ka, a], [kb, b]) => pinnedAt(ka) - pinnedAt(kb) || a.prominence - b.prominence || ka.localeCompare(kb))
    .map(([, b]) => b.best)
}

/** Columns for square provider cards about 128px wide, with the grid's 14px gap. */
export function providerColumns(width: number): number {
  if (width <= 0)
    return 5
  return Math.max(4, Math.min(12, Math.floor((width + 14) / (128 + 14))))
}

/**
 * How many cards to show so every row the strip draws is full: `rows` whole
 * rows when there are enough, otherwise as many whole rows as there are, and
 * only a short list gets a short row. A lone card on a second line is what
 * this exists to prevent.
 */
export function fillRows(available: number, cols: number, rows: number): number {
  if (available <= 0 || cols <= 0)
    return 0
  if (available >= cols * rows)
    return cols * rows
  if (available >= cols)
    return Math.floor(available / cols) * cols
  return available
}
