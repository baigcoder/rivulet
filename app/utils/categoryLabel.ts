/**
 * Provider group names arrive as `AF - Canal+ Africa - Live` or
 * `ARG - Argentina`. The letters are the provider's folder, not always an
 * ISO country, so they are a badge and never a flag. `Live` on the end is
 * the same stamp on every row.
 */
function lettersAt(name: string, n: 2 | 3): boolean {
  if (name.length < n + 3)
    return false
  for (let i = 0; i < n; i++) {
    const c = name.charCodeAt(i)
    if (c < 65 || c > 90)
      return false
  }
  return name.startsWith(' - ', n)
}

export function parseCategoryName(raw: string): { code: string | null, label: string } {
  const trimmed = raw.trim()
  const n: 0 | 2 | 3 = lettersAt(trimmed, 3) ? 3 : lettersAt(trimmed, 2) ? 2 : 0
  let label = n ? trimmed.slice(n + 3).trim() : trimmed
  if (label.endsWith(' - Live'))
    label = label.slice(0, -7).trim()
  else if (label.endsWith(' Live'))
    label = label.slice(0, -5).trim()
  return { code: n ? trimmed.slice(0, n) : null, label: label || trimmed }
}

/** `ALL SPORTS`, `ALL MOVIES` — the provider's catch-alls, not a country folder. */
export function isBundleCategory(name: string): boolean {
  return /^all[\s_-]/i.test(name.trim())
}

function prettyBundle(label: string): string {
  return label.replace(/[a-z0-9]+/gi, word => {
    if (/^(?:4K|8K|HD|FHD|UHD|SD|TV|VOD)$/i.test(word))
      return word.toUpperCase()
    return word.charAt(0).toUpperCase() + word.slice(1).toLowerCase()
  })
}

export function categoryLabel(raw: string): string {
  const parsed = parseCategoryName(raw)
  if (isBundleCategory(raw))
    return prettyBundle(parsed.label)
  return parsed.label
}
