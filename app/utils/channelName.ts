/** Quality tokens providers embed in channel titles. */
const QUALITY = ['4K', 'UHD', 'FHD', 'HEVC', 'HD', 'SD'] as const
const PACKAGE = /^(?:VIP|MULTI)$/i

export interface ParsedChannelName { name: string, quality: string }

/** Strip bitrate/package noise from a provider channel name. */
export function parseChannelName(raw: string, fallback = 'Channel'): ParsedChannelName {
  const trimmed = (raw || '').trim()
  let quality = ''
  const kept: string[] = []
  for (const word of trimmed.split(/\s+/)) {
    const bare = word.replace(/^[[(]|[\])]$/g, '')
    const hit = QUALITY.find(q => q.toLowerCase() === bare.toLowerCase())
    if (hit) {
      if (!quality || QUALITY.indexOf(hit) < QUALITY.indexOf(quality as typeof QUALITY[number]))
        quality = hit
      continue
    }
    if (PACKAGE.test(bare))
      continue
    kept.push(word)
  }
  const name = kept.join(' ').replace(/\s{2,}/g, ' ').trim()
  return { name: name || trimmed || fallback, quality }
}

/** Two letters for a logo placeholder. */
export function channelInitials(name: string): string {
  const clean = name.replace(/[^a-z0-9\s]/gi, ' ').trim()
  const words = clean
    .split(/\s+/)
    .filter(w => w.length > 0 && !/^(?:tv|channel|live|hd|sd|fhd|4k|uhd)$/i.test(w))
  const first = words[0] || clean
  const second = words[1]
  if (first && second && first[0] && second[0])
    return (first[0] + second[0]).toUpperCase()
  return (first || clean || 'TV').slice(0, 2).toUpperCase()
}
