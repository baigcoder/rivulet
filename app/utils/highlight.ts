/**
 * Highlight search matches in a string by wrapping matched substrings
 * in a `<mark>` element. Case-insensitive, safe against regex metachars
 * in the query.
 *
 *   highlightMatch('BBC News', 'bbc')    → '<mark>BBC</mark> News'
 *   highlightMatch('a.b', 'a.b')         → '<mark>a.b</mark>'  (literal)
 *   highlightMatch('no match', 'zzz')    → 'no match'         (no match)
 *
 * Returns the input as-is when the query is empty or contains only
 * whitespace — nothing to highlight, and emitting an empty `<mark></mark>`
 * is a worse result.
 */
export function highlightMatch(text: string, query: string): string {
  const q = query.trim()
  if (!q)
    return text
  const escaped = q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const re = new RegExp(`(${escaped})`, 'gi')
  return text.replace(re, '<mark class="bg-primary/30 text-inherit rounded-sm px-0.5">$1</mark>')
}
