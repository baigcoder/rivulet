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

/** Title and year for the detail page and for a TMDB query. */
export function vodDisplayName(title: string): { name: string, year: string } {
  const year = title.match(YEAR)?.[1] ?? ''
  const name = stripProviderPrefix(title).replace(YEAR, '').replace(/\s+/g, ' ').trim()
  return { name: name || title.trim(), year }
}
