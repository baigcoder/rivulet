/**
 * Finding something to play, and handing it to the local torrent engine.
 *
 * The app indexes nothing and ships with no sources: this file searches only
 * the servers a user has added by hand. A source is a URL that answers
 * `GET {base}/stream/{movie|series}/{imdbId}.json` with a `streams` array — an
 * open, documented protocol with several independent implementations, so what
 * a given source returns is between its operator and the user who added it.
 *
 * A list of URLs, not a plugin runtime. The protocol is plain HTTP
 * and JSON, so a source needs no sandbox, no manifest, and runs no code of
 * ours — the fan-out below is the entire "plugin system".
 */
import { deviceCodecs, hasNativePlayer } from './htmlvideo'

/**
 * Sources to search, in the order they were added. Empty until the user adds
 * one; the settings store pushes the list on change, the same way it pushes
 * the download folder to `setDownloadDir`.
 */
let sources: string[] = []

// A function, not a constant: it is built when this module loads, before `$t`
// has a locale to read — see SECTIONS in the settings store.
export const NO_SOURCES = () => $t('No sources configured. Add one in Settings → Sources.')

export function setSources(urls: string[]) {
  // Trailing slashes would produce `//stream/…`, which some servers 404.
  sources = urls.map(u => u.trim().replace(/\/+$/, '')).filter(Boolean)
}

/**
 * The same list, for the subtitle search. One addon protocol serves `/stream/`
 * and `/subtitles/` off the same base, so an addon the user already trusts for
 * releases is asked about subtitles too — still nothing this app added itself.
 */
export function configuredSources() {
  return sources
}

/**
 * What a user actually has on the clipboard — or what arrived on a `rivulet://`
 * link — turned into a base URL we can append `/stream/…` to. Nobody copies a
 * bare origin: an addon hands out a scheme link or a `…/manifest.json` URL, and
 * a configured one carries its settings in the path
 * (`https://host/opt=a,b/manifest.json`), which is part of the base and has to
 * survive.
 *
 * Returns '' for anything that isn't a URL, so the caller can say so.
 */
export function normalizeSource(input: string): string {
  const url = input.trim()
    // `rivulet://` is our own deep link, `stremio://` is what addon pages
    // already publish; both name an https server in the same shape.
    .replace(/^(?:rivulet|stremio):\/\//i, 'https://')
    .replace(/\/manifest\.json(?:[?#].*)?$/i, '')
    .replace(/\/+$/, '')

  return /^https?:\/\/[^\s/]+/i.test(url) ? url : ''
}

/** Public trackers, so a magnet without any of its own still finds peers fast. */
const TRACKERS = [
  'udp://tracker.opentrackr.org:1337/announce',
  'udp://open.demonii.com:1337/announce',
  'udp://open.stealth.si:80/announce',
  'udp://tracker.torrent.eu.org:451/announce',
  'udp://exodus.desync.com:6969/announce',
  'udp://open.tracker.cl:1337/announce',
]

/**
 * Streaming preference. 1080p is the sweet spot; 4k needs more bandwidth than
 * most connections give a torrent, so it sits below 720p.
 */
const QUALITY_ORDER = ['1080p', '720p', '4k', '2160p', '480p']

/**
 * Past this a release is a remux or a needlessly fat encode: the same picture
 * at several times the bitrate, which is bandwidth a torrent stream doesn't
 * have. A 1080p feature is done in ~3 GB, so the 12 GB copy of it stops
 * outranking the small one just because it has more seeders.
 */
const SWEET_BYTES: Record<string, number> = {
  '1080p': 6 * 1024 ** 3,
  '720p': 3 * 1024 ** 3,
  '4k': 20 * 1024 ** 3,
  '2160p': 20 * 1024 ** 3,
  '480p': 2 * 1024 ** 3,
}

/** Remuxes stream badly on anything but a LAN — a 60 GB movie never keeps up. */
const MAX_BYTES = 25 * 1024 ** 3

const VIDEO_EXT = /\.(?:mkv|mp4|webm|avi|mov|m4v|ts|m2ts|flv|wmv)$/i

/**
 * Text subtitles only. VobSub (`.sub` + `.idx`) is a pair of files mpv can only
 * pair up on a local disk, and it's one http URL each here — a picker entry that
 * could never play is worse than no entry.
 */
const SUBTITLE_EXT = /\.(?:srt|ass|ssa|vtt)$/i

/** Never worth playing, however many seeders it has. */
const JUNK = /\b(?:cam|hdcam|ts|hdts|telesync|telecine|scr|screener|r5)\b/i

const UNITS: Record<string, number> = { B: 1, KB: 1024, MB: 1024 ** 2, GB: 1024 ** 3, TB: 1024 ** 4 }

/** Resolution token in a release or addon label — "1080p", "4k HDR", … */
const QUALITY_TOKEN = /\b(?:2160p|4k(?:\s*(?:dv|hdr)[^\s,|]*)?|1080p|720p|480p)\b/i

/**
 * What tier a stream is. Debrid addons often put the resolution only in the
 * release name, not in `name`'s second line — MediaFusion is one of them.
 */
export function releaseQuality(raw: { quality?: string, rawName?: string, name?: string, title?: string, file?: string | null }) {
  const preset = (raw.quality ?? '').trim()
  if (preset && QUALITY_TOKEN.test(preset))
    return preset
  for (const line of (raw.rawName ?? '').split('\n').map(s => s.trim()).filter(Boolean)) {
    const m = line.match(QUALITY_TOKEN)
    // "MediaFusion\n1080p" uses the second line; a lone "MediaFusion" does not.
    if (m && !/^(?:mediafusion|stremio|torrentio|comet|debrid|example)$/i.test(line))
      return m[0].trim()
  }
  for (const text of [raw.name, raw.title, raw.file ?? '']) {
    if (!text)
      continue
    const m = text.match(QUALITY_TOKEN)
    if (m)
      return m[0].trim()
  }
  return ''
}

/**
 * One result a source returned. Most are torrents, but the same protocol also
 * carries plain HTTP links — that is what a debrid addon answers with, having
 * already fetched the torrent on its own servers, and what an addon that hosts
 * its own files answers with. A debrid row often has both: the URL is for
 * Play, the hash (sometimes only inside that URL) is for Download.
 */
export interface Release {
  /** Release name, e.g. "Sintel 2010 1080p BluRay x264". */
  name: string
  hash: string
  /**
   * A link to play directly, '' for a torrent. It needs no swarm, no metadata
   * round trip and no disk, so a release that has one is preferred within its
   * quality tier and is exempt from the storage budget.
   */
  url: string
  /** Index of the wanted file inside the torrent, when the source knows it. */
  fileIdx: number | null
  /**
   * The file inside the torrent this result points at, when the source names one.
   * Only season packs have it, and it's the only place the episode is spelled
   * out — the release name above says "S01", not which episode.
   */
  file: string | null
  seeders: number
  /** Human size of the file we'd stream, e.g. "2.1 GB". */
  size: string
  bytes: number
  /** Whatever the source labelled the result's origin with, if anything. */
  source: string
  /** "1080p", "720p", "4k DV | HDR", … as labelled by the source. */
  quality: string
  magnet: string
  /**
   * Which configured source answered with this release, as the base URL the
   * fan-out asked. Set by `findReleases`, and it is the addon's identity in
   * the player's server menu — the "origin" on the stats line above is only
   * whatever label that addon printed.
   */
  via?: string
}

interface RawStream {
  name?: string
  title?: string
  /** What `title` was renamed to; addons emit one or the other. */
  description?: string
  infoHash?: string
  /** Same field, other addons. */
  infohash?: string
  fileIdx?: number
  url?: string
  sources?: string[]
  behaviorHints?: { videoSize?: number, filename?: string, bingeGroup?: string }
}

const BTIH = /(?:urn:)?btih:([a-f0-9]{40}|[a-z2-7]{32})/i

function hex40s(text: string): string[] {
  return text.match(/[a-f0-9]{40}/gi) ?? []
}

/**
 * Info hash a stream is actually a torrent for. Addons hide it: `infoHash`,
 * lowercase `infohash`, a `dht:` source, a magnet in `url`, or — the debrid
 * case — a 40-hex segment in the resolve URL with no `infoHash` field at all.
 * That last one is what made every Releases row look like Direct and made
 * Download save an HTTP file instead of starting the engine.
 */
function streamHash(raw: RawStream, rawUrl: string): string {
  const listed = (raw.infoHash || raw.infohash || '').trim()
  if (listed)
    return listed
  const dht = (raw.sources ?? []).find(s => s.startsWith('dht:'))
  if (dht)
    return dht.slice(4)
  const magnet = rawUrl.startsWith('magnet:')
    ? rawUrl
    : (raw.sources ?? []).find(s => s.startsWith('magnet:')) ?? ''
  const fromMagnet = magnet.match(BTIH)?.[1]
  if (fromMagnet)
    return fromMagnet
  const fromUrl = hashFromUrl(rawUrl)
  if (fromUrl)
    return fromUrl
  return hex40s(raw.behaviorHints?.bingeGroup ?? '')[0] ?? ''
}

/**
 * Debrid resolve URLs look like `/resolve/{service}/{token}/{hash}/…`.
 * The token is often also 40 hex, so the first match would steal the API
 * key. Prefer the `{service}/{token}/{hash}` layout; otherwise the last
 * 40-hex path segment.
 */
function hashFromUrl(url: string): string {
  if (!url || url.startsWith('magnet:'))
    return ''
  let path = url
  try {
    path = new URL(url).pathname
  }
  catch { /* not absolute; still scan it as a path */ }
  const resolve = path.match(/\/resolve\/[^/]+\/[^/]+\/([a-f0-9]{40})(?:\/|$)/i)
  if (resolve)
    return resolve[1]!
  const found = hex40s(path)
  return found.length ? found[found.length - 1]! : ''
}

/** `fileIdx` on the stream, or the `/null/0/filename` slot debrid URLs use. */
function streamFileIdx(raw: RawStream, rawUrl: string): number | null {
  if (typeof raw.fileIdx === 'number' && raw.fileIdx >= 0)
    return raw.fileIdx
  const m = rawUrl.match(/\/(?:null|\d+)\/(\d+)\/[^/?#]+$/i)
  return m ? Number(m[1]) : null
}

function magnetFor(hash: string, name: string, sources?: string[]) {
  // A stream's `sources` are prefixed entries: "tracker:udp://…", "dht:<hash>".
  const own = (sources ?? []).filter(s => s.startsWith('tracker:')).map(s => s.slice(8))
  const trackers = [...new Set([...own, ...TRACKERS])]
  const tr = trackers.map(t => `&tr=${encodeURIComponent(t)}`).join('')
  return `magnet:?xt=urn:btih:${hash}&dn=${encodeURIComponent(name)}${tr}`
}

/**
 * One stream from a source -> one release. Everything but the info hash lives
 * in a multi-line display title whose stats line reads
 * `👤 375 💾 928.25 MB ⚙️ origin`, so it gets parsed back out here.
 */
export function toRelease(raw: RawStream, base = ''): Release | null {
  // Either something to fetch or something to open. A stream with neither hands
  // playback to another app or another protocol, which is not ours to follow.
  // Debrid addons sometimes emit a path (`/playback/…`) against their own host.
  let rawUrl = (raw.url ?? '').trim()
  if (rawUrl && !/^https?:\/\//i.test(rawUrl) && !rawUrl.startsWith('magnet:') && base) {
    try {
      const root = new URL(base.endsWith('/') ? base : `${base}/`)
      rawUrl = rawUrl.startsWith('/')
        ? new URL(`${root.pathname.replace(/\/+$/, '')}${rawUrl}`, root.origin).href
        : new URL(rawUrl, root).href
    }
    catch {
      rawUrl = ''
    }
  }
  const hash = streamHash(raw, rawUrl)
  const url = /^https?:\/\//i.test(rawUrl) ? rawUrl : ''
  if (!hash && !url)
    return null

  const title = raw.description || raw.title || ''
  const lines = title.split('\n').map(line => line.trim())
  const name = lines[0] || raw.behaviorHints?.filename || (raw.name ?? '').split('\n')[0] || (url ? 'Stream' : '')
  if (!name || JUNK.test(name))
    return null

  const [, amount, unit] = title.match(/💾\s*([\d.]+)\s*([KMGT]?B)/) ?? []
  // Debrid addons have already resolved the file, so they tend to give its
  // exact length here instead of drawing the stats line a torrent needs.
  const bytes = amount ? Number(amount) * (UNITS[unit!] ?? 0) : raw.behaviorHints?.videoSize ?? 0

  return {
    name,
    hash,
    url,
    fileIdx: streamFileIdx(raw, rawUrl),
    file: lines.slice(1).find(line => VIDEO_EXT.test(line)) ?? null,
    seeders: Number(title.match(/👤\s*(\d+)/)?.[1] ?? 0),
    size: amount ? `${amount} ${unit}` : bytes ? bytesText(bytes) : '',
    bytes,
    // "⚙️" is a gear plus a variation selector — match the gear, skip whatever
    // decoration follows it, and take the next word.
    source: title.match(/⚙\S*\s+(\S+)/)?.[1] ?? 'unknown',
    quality: releaseQuality({ rawName: raw.name, name, title, file: lines.slice(1).find(line => VIDEO_EXT.test(line)) ?? null }),
    magnet: hash ? magnetFor(hash, name, raw.sources) : '',
  }
}

/** The host of a configured source URL — the identity the UI shows. */
export function sourceHost(via: string): string {
  try {
    return new URL(via).host
  }
  catch {
    return via
  }
}

/** What makes two results the same result — a hash for a torrent, the link itself for a link. */
export function releaseKey(r: Release) {
  return r.url || r.hash
}

/**
 * Filename for saving a Direct link to the download folder. The release
 * name is what the user saw; a path in it would write outside that folder.
 */
export function releaseFileName(t: { name: string, file?: string | null, url?: string }) {
  const raw = (t.file || t.name || 'download').split(/[/\\]/).pop() || 'download'
  const cleaned = [...raw]
    .map(c => (c.charCodeAt(0) < 32 || '<>:"|?*'.includes(c)) ? '_' : c)
    .join('')
    .replace(/\.+$/g, '')
    .trim()
    .slice(0, 180)
  const base = cleaned || 'download'
  if (VIDEO_EXT.test(base))
    return base
  let ext = 'mkv'
  if (t.url) {
    try {
      const found = new URL(t.url).pathname.match(VIDEO_EXT)
      if (found)
        ext = found[0].slice(1)
    }
    catch { /* not a url */ }
  }
  return `${base}.${ext}`
}

function rank(t: Release) {
  const i = QUALITY_ORDER.findIndex(q => t.quality.toLowerCase().startsWith(q))
  return i === -1 ? QUALITY_ORDER.length : i
}

/** More bytes than this tier needs to look good — a bigger bill for the same picture. */
export function isBloated(t: Release) {
  const cap = SWEET_BYTES[QUALITY_ORDER[rank(t)] ?? ''] ?? MAX_BYTES
  return t.bytes > cap
}

/**
 * What a release name says it carries, and how to ask this device about each.
 *
 * A name is all a source gives us to go on, but it is only half the question —
 * the other half is which player is behind the controls, and they differ wildly:
 *
 *   - `android` is a MediaCodec mime type, answered from the platform's own
 *     decoder list (see `deviceCodecs`). This is why the same release is fine on
 *     an Android TV box, which nearly always has Dolby and HEVC in hardware, and
 *     silent on a mid-range phone, which often has neither.
 *   - `mime` is what MediaSource is asked where the player is the webview's
 *     `<video>` — a browser, i.e. `bun run dev`. TrueHD has none because there
 *     is no MSE string for it, and no webview has ever decoded one.
 */
const CODECS = [
  { re: /\be-?ac-?3\b|\bdd[p+]/i, android: 'audio/eac3', mime: 'audio/mp4; codecs="ec-3"' },
  { re: /\bac-?3\b|\bdd5[\W_]?1\b/i, android: 'audio/ac3', mime: 'audio/mp4; codecs="ac-3"' },
  { re: /\bdts/i, android: 'audio/vnd.dts', mime: 'audio/mp4; codecs="dtsc"' },
  { re: /\btruehd\b|\batmos\b/i, android: 'audio/true-hd', mime: '' },
  { re: /\bx265\b|\bh\.?265\b|\bhevc\b/i, android: 'video/hevc', mime: 'video/mp4; codecs="hvc1.1.6.L93.B0"' },
  { re: /\bav1\b/i, android: 'video/av01', mime: 'video/mp4; codecs="av01.0.05M.08"' },
  // Main 10. MediaCodec doesn't split HEVC by profile, and in practice a device
  // with a hardware HEVC decoder has the 10-bit profile too.
  { re: /\b10.?bits?\b/i, android: 'video/hevc', mime: 'video/mp4; codecs="hvc1.2.4.L120.B0"' },
]

/**
 * No codec to ask about — a remux is a full-bitrate disc, which is a bandwidth
 *  problem before it is a decoding one.
 */
const RISKY = /\bremux\b/i

/**
 * Can the player on this device decode this? `null` where there is nobody to ask
 * — a `bun run check:*` with no browser around it — and the caller falls back to
 * the release name alone, which is the cautious answer.
 */
function canDecode(c: { android: string, mime: string }): boolean | null {
  const codecs = deviceCodecs()
  if (codecs)
    return codecs.has(c.android)
  const mse = (globalThis as { MediaSource?: { isTypeSupported?: (type: string) => boolean } }).MediaSource
  if (c.mime && mse?.isTypeSupported)
    return mse.isTypeSupported(c.mime)
  return null
}

export function isAwkward(t: Release) {
  // mpv carries its own ffmpeg and cares about none of this.
  if (hasNativePlayer())
    return false
  const name = `${t.name} ${t.quality}`
  return RISKY.test(name) || CODECS.some(c => c.re.test(name) && canDecode(c) !== true)
}

/**
 * The list, best first. `pickBest` is this plus `[0]`, and the player's server
 * and quality menus are the whole of it — so both places order candidates
 * identically by construction.
 *
 * When `allowTorrents` is true, torrents are preferred over direct links so the
 * torrent engine downloads progressively while you watch (offline copy).
 * When false (stream-only mode), only direct links are considered anyway.
 */
export function ranked(list: Release[], maxBytes = MAX_BYTES, compatible = false, allowTorrents = false): Release[] {
  const limit = Math.min(MAX_BYTES, maxBytes)
  return [...list]
    // Neither test applies to a link: there is no swarm to have seeders, and
    // nothing is written to the disk the budget is protecting.
    .filter(t => !!t.url || (t.seeders > 0 && (!t.bytes || t.bytes <= limit)))
    .sort((a, b) =>
      rank(a) - rank(b)
      || (compatible ? Number(isAwkward(a)) - Number(isAwkward(b)) : 0)
      || Number(isBloated(a)) - Number(isBloated(b))
      || (allowTorrents ? Number(!!a.url) - Number(!!b.url) : Number(!a.url) - Number(!b.url))
      || b.seeders - a.seeders)
}

/**
 * Best quality tier we'd actually stream, then the copies of it that aren't
 * bloated, and within those the most seeders. `maxBytes` is the device's storage
 * budget: a release that can't fit on the disk is no use however good it is.
 *
 * `compatible` breaks ties towards what this device can actually decode, which
 * `isAwkward` now asks the platform rather than guessing from the name — so a TV
 * box keeps the Dolby copy it can play and a phone without the decoder doesn't.
 *
 * It stays *below* the quality tier: dropping a whole tier to dodge a codec is
 * the wrong trade now that the check is accurate, since anything it demotes is
 * something this device genuinely cannot play at any resolution.
 *
 * When `allowTorrents` is true, torrents are preferred over direct links so the
 * torrent engine downloads progressively while you watch (offline copy).
 * When false, a direct link wins the last tiebreak before seeders: same picture,
 * same bitrate, but it starts at once and nothing has to be kept on the disk.
 */
export function pickBest(list: Release[], maxBytes = MAX_BYTES, compatible = false, allowTorrents = false): Release | null {
  return ranked(list, maxBytes, compatible, allowTorrents)[0] ?? null
}

/**
 * What Play opens.
 *
 * Engine on: a magnet, so playback starts from the torrent engine and the rest
 * of the file keeps downloading while you watch. Engine off: a Direct link
 * only — nothing is added to the engine.
 */
export function pickPlay(list: Release[], maxBytes = MAX_BYTES, compatible = false, allowTorrents = false): Release | null {
  const pool = hasNativePlayer() ? list : withoutUhd(list)
  if (allowTorrents)
    return pickBest(pool, maxBytes, compatible, true)
  return pickBest(pool.filter(t => !!t.url), maxBytes, compatible, false)
}

/** Drop 4K when a lighter copy exists. Empty result means the list was 4K-only. */
export function withoutUhd(list: Release[]): Release[] {
  const hd = list.filter(t => !/\b(?:2160p|4k)\b/i.test(`${t.quality} ${t.name}`))
  return hd.length ? hd : list
}

const AUDIO_LANGS = [
  { re: /\bhindi\b|\bhin\b/i, code: 'hi' },
  { re: /\benglish\b|\beng\b/i, code: 'en' },
  { re: /\btamil\b|\btam\b/i, code: 'ta' },
  { re: /\btelugu\b|\btel\b/i, code: 'te' },
  { re: /\bmalayalam\b/i, code: 'ml' },
  { re: /\bkannada\b/i, code: 'kn' },
  { re: /\bspanish\b|\bspa\b|\besp\b/i, code: 'es' },
  { re: /\bfrench\b|\bfre\b|\bfra\b/i, code: 'fr' },
  { re: /\bjapanese\b|\bjpn\b/i, code: 'ja' },
  { re: /\bkorean\b|\bkor\b/i, code: 'ko' },
] as const

/** Languages a release name claims to carry — "Dual Audio Hindi English". */
export function releaseLangs(text: string): string[] {
  const out: string[] = []
  for (const l of AUDIO_LANGS) {
    if (l.re.test(text) && !out.includes(l.code))
      out.push(l.code)
  }
  return out
}

/**
 * Stream-only mode's answer when there is nothing to stream. Distinct from a
 * plain error because the watch page offers "turn torrents back on" beside it —
 * which is the one fix, and the toggle lives one setting away.
 */
export class NoServerStream extends Error {
  /**
   * Hosts of the sources that answered with downloads only — shown by name on
   * the explainer, so "add a Stremio URL" has something concrete to point at.
   */
  readonly viaNames: string[]
  constructor(message: string, viaNames: string[] = []) {
    super(message)
    this.viaNames = viaNames
  }
}

/**
 * Just the direct links, best first: what playback resolves through when
 * torrent downloads are switched off, and the pool the player's server and
 * quality menus draw from either way.
 *
 * When `allowTorrents` is true, torrents at the same quality tier are included
 * so the failover/alternatives menus show torrents too.
 */
export function serverCandidates(releases: Release[], maxBytes = MAX_BYTES, compatible = false, allowTorrents = false): Release[] {
  const pool = allowTorrents ? releases : releases.filter(t => !!t.url)
  return ranked(pool, maxBytes, compatible, allowTorrents)
}

/** Patient default; racing mode shortens it so a dead server fails over in seconds. */
const SOURCE_TIMEOUT = 20_000
const RACE_TIMEOUT = 8_000

async function searchOne(base: string, path: string, timeoutMs = SOURCE_TIMEOUT): Promise<Release[]> {
  const res = await fetch(base + path, { signal: AbortSignal.timeout(timeoutMs) })
  if (!res.ok)
    throw new Error(`${base} answered HTTP ${res.status}`)

  const data = await res.json() as { streams?: RawStream[] }
  return (data.streams ?? []).flatMap(s => toRelease(s, base) ?? [])
}

/**
 * Everything the configured sources know for a movie, or for one episode of a
 * show.
 *
 * Two speeds, one machinery. The patient search waits for every server so the
 * caller sees one complete answer; the racing search cuts loose as soon as the
 * first healthy server answers (plus a short grace window), hands those back,
 * and streams anything slower into `onLate` afterwards — which is how playback
 * starts on the fastest server instead of the slowest source's deadline.
 */

function searchPath(imdbId: string, season: number, episode: number) {
  const series = season > 0 && episode > 0
  const id = series ? `${imdbId}:${season}:${episode}` : imdbId
  return `/stream/${series ? 'series' : 'movie'}/${id}.json`
}

async function runSources(
  path: string,
  wait: { mode: 'all' } | { mode: 'first', graceMs: number, onBatch?: (releases: Release[]) => void },
): Promise<{ releases: Release[], rest: Promise<Release[]> }> {
  if (!sources.length)
    throw new Error(NO_SOURCES())

  // Racing mode shortens the per-source leash: a server that hasn't answered
  // in eight seconds is a dead candidate as far as this playback is concerned.
  const timeoutMs = wait.mode === 'first' ? RACE_TIMEOUT : SOURCE_TIMEOUT
  const tasks = sources.map(base => searchOne(base, path, timeoutMs))
  const batches: (Release[] | null)[] = sources.map(() => null)
  let anyAnswered = false
  let anyFailed = false

  let firstUseful!: () => void
  const useful = new Promise<void>(resolve => {
    firstUseful = resolve
  })

  // Merge whatever landed, in the order sources were added — that order is the
  // preference order, and it also decides whose copy a duplicate belongs to.
  const seen = new Map<string, Release>()
  const merge = (i: number) => {
    const batch = batches[i]
    if (!batch)
      return []
    batches[i] = null
    const added: Release[] = []
    for (const t of batch) {
      if (!seen.has(releaseKey(t))) {
        seen.set(releaseKey(t), t)
        t.via = sources[i]
        added.push(t)
      }
    }
    return added
  }
  const takeLanded = () => {
    for (let i = 0; i < sources.length; i++)
      merge(i)
  }

  /**
   * Only set once the first wave has been merged, so nothing can jump the
   * preference order. After that a straggler is reported the moment it lands
   * rather than at the end of the batch — the caller waiting for a stream URL
   * has no reason to sit through the slowest source to hear about it.
   */
  let closed = false
  const onBatch = wait.mode === 'first' ? wait.onBatch : undefined

  const settled = tasks.map(async (task, i) => {
    try {
      batches[i] = await task
      anyAnswered = true
      // An empty 200 (a source whose filters matched nothing) is not a
      // reason to start playback — wait for a source that actually has streams.
      if (batches[i]!.length)
        firstUseful()
      if (closed && onBatch) {
        const added = merge(i)
        if (added.length)
          onBatch(added)
      }
    }
    catch {
      anyFailed = true
    }
  })

  // First *useful* answer starts the clock; everyone else gets `graceMs` to
  // join before the window shuts. In 'all' mode the window never shuts early.
  const opened = wait.mode === 'first'
    ? Promise.race([useful, Promise.all(settled)])
    : Promise.all(settled)
  await Promise.race([
    opened.then(() => new Promise(r => setTimeout(r, wait.mode === 'first' ? wait.graceMs : 0))),
    Promise.all(tasks.map(t => t.catch(() => []))),
  ])

  takeLanded()
  closed = true

  if (!seen.size && !anyAnswered && anyFailed)
    throw new Error($t('No source answered.'))

  const releasedKeys = new Set(seen.keys())
  const rest = Promise.all(tasks.map(t => t.catch(() => []))).then(() => {
    takeLanded()
    return [...seen.values()].filter(t => !releasedKeys.has(releaseKey(t)))
  })

  return { releases: [...seen.values()], rest }
}

/** Patient: every source answers (or times out) before anything is returned. */
export async function findReleases(imdbId: string, season = 0, episode = 0): Promise<Release[]> {
  const { releases } = await runSources(searchPath(imdbId, season, episode), { mode: 'all' })
  return releases
}

/**
 * Racing: resolve with the first wave of answers, stream the stragglers into
 * `onLate` as they land (already deduped against what was handed back).
 */
export async function findReleasesFast(
  imdbId: string,
  season: number,
  episode: number,
  options: { graceMs?: number, onLate?: (releases: Release[]) => void, needUrl?: boolean, needMagnet?: boolean } = {},
): Promise<Release[]> {
  // Resolves with everything landed so far the moment a straggler brings a
  // stream URL. Built before the search so it can be handed in as `onBatch`.
  const landed: Release[] = []
  let urlLanded!: (releases: Release[]) => void
  const gotUrl = new Promise<Release[]>(resolve => {
    urlLanded = resolve
  })
  let magnetLanded!: (releases: Release[]) => void
  const gotMagnet = new Promise<Release[]>(resolve => {
    magnetLanded = resolve
  })

  const { releases, rest } = await runSources(
    searchPath(imdbId, season, episode),
    {
      mode: 'first',
      graceMs: options.graceMs ?? 50,
      onBatch: (added: Release[]) => {
        landed.push(...added)
        if (added.length)
          urlLanded([...landed])
        if (added.some(r => r.magnet))
          magnetLanded([...landed])
      },
    },
  )
  let out = releases
  // Magnet-only first wave is common: the debrid host resolves a beat later.
  // Waiting on `rest` alone means waiting for the slowest source of the lot,
  // so the first URL to land ends the wait; the timer is only the give-up
  // bound for when no source has one at all.
  if (options.needUrl && !out.some(r => r.url)) {
    const late = await Promise.race([
      gotUrl,
      rest,
      new Promise<Release[]>(resolve => setTimeout(resolve, 2000, [])),
    ])
    if (late.length)
      out = [...out, ...late]
  }
  // Torrent-engine mode is not merely a ranking preference. A fast Direct
  // source used to end the race before a slower magnet source could answer,
  // silently turning "Torrent engine" into Direct play and leaving Downloads
  // empty. Give a magnet the same short chance that Direct-only mode gives a
  // URL; use the Direct result only when no torrent source responds.
  if (options.needMagnet && !out.some(r => r.magnet)) {
    const late = await Promise.race([
      gotMagnet,
      rest,
      new Promise<Release[]>(resolve => setTimeout(resolve, 2000, [])),
    ])
    if (late.length)
      out = [...out, ...late]
  }
  const seen = new Set(out.map(releaseKey))
  void rest.then(late => {
    const extra = late.filter(r => !seen.has(releaseKey(r)))
    if (extra.length)
      options.onLate?.(extra)
  })
  return out
}

// --- Local engine -------------------------------------------------------------
// The librqbit HTTP + streaming server the Tauri backend starts on boot
// (src-tauri/src/lib.rs). Everything below is plain fetch() against it.

export const ENGINE = 'http://127.0.0.1:3030'

export interface EngineFile {
  name: string
  length: number
  included: boolean
  /** Path parts relative to the torrent's output folder, last one the file. */
  components?: string[]
}

export interface TorrentStats {
  /** "initializing" | "live" | "paused" | "error" */
  state: string
  error: string | null
  progress_bytes: number
  uploaded_bytes: number
  total_bytes: number
  finished: boolean
  /** Bytes we hold of each file, in the torrent's own file order. */
  file_progress: number[]
  live: null | {
    download_speed: { mbps: number, human_readable: string }
    upload_speed: { mbps: number, human_readable: string }
    time_remaining: { human_readable: string } | null
    snapshot: { peer_stats: { live: number, seen: number } }
  }
}

/** A torrent as the engine lists it. */
export interface EngineTorrent {
  id: number
  info_hash: string
  name: string | null
  output_folder: string
  files?: EngineFile[]
  stats?: TorrentStats
  /** How many pieces the torrent is cut into. See `pieceMap`. */
  total_pieces?: number
}

/**
 * Where new torrents are written, from the storage setting. Empty means the
 * engine's own default (a folder in the app's cache dir). It lives here rather
 * than being threaded through every caller because `addTorrent` is the only
 * place a torrent is ever created — the settings store pushes it on change.
 */
let downloadDir = ''

export function setDownloadDir(path: string) {
  downloadDir = path.trim()
}

function packAdded(t: EngineTorrent) {
  return { id: t.id, details: { name: t.name, info_hash: t.info_hash, files: t.files ?? null } }
}

/** The engine already knows this hash — files may still be empty (metadata in flight). */
async function listedTorrent(hash: string): Promise<EngineTorrent | null> {
  const byHash = await torrentDetails(canonHash(hash))
  if (typeof byHash?.id === 'number')
    return byHash
  return (await listTorrents().catch(() => [])).find(t => hashesMatch(t.info_hash, hash)) ?? null
}

export async function addTorrent(magnet: string) {
  const hash = magnetHash(magnet)
  if (hash) {
    const existing = await listedTorrent(hash)
    if (existing)
      return packAdded(existing)
  }
  // A hash already in the list is one librqbit is fetching. A second POST
  // waits the full `timeout_ms` for metadata of a torrent it already holds —
  // the "Fetching metadata from peers…" first Play that then goes blank,
  // while Back + Play finds the copy and streams.
  const alreadyListed = !!hash && !!(await listedTorrent(hash))
  // Only new torrents move: the engine remembers an existing one's folder, and
  // its data is already sitting in it.
  const folder = downloadDir ? `&output_folder=${encodeURIComponent(downloadDir)}` : ''

  interface Added { id: number, details: { name: string | null, info_hash: string, files: EngineFile[] | null } }
  let posted: Added | null = null
  let postError: Error | null = null
  if (!alreadyListed) {
    void fetch(`${ENGINE}/torrents?overwrite=true&timeout_ms=180000${folder}`, { method: 'POST', body: magnet })
      .then(async res => {
        if (!res.ok)
          throw new Error($t('Torrent engine said {status}: {reason}', { status: res.status, reason: await res.text() }))
        const added = await res.json() as { id: number | null, details: Added['details'] }
        if (added.id == null)
          throw new Error($t('The torrent engine accepted the magnet but gave it no id.'))
        posted = { ...added, id: added.id }
      })
      .catch(e => {
        const err = e instanceof TypeError
          ? new Error($t('Torrent engine offline. Launch the native desktop or Android app to play torrents.'))
          : e instanceof Error ? e : new Error(String(e))
        // The list can lag the add. A second POST then 400s "already live"
        // for a hash we are about to see — keep polling, do not throw.
        if (hash && /already live/i.test(err.message))
          return
        postError = err
      })
  }

  const deadline = Date.now() + 180_000
  while (Date.now() < deadline) {
    if (hash) {
      const existing = await listedTorrent(hash)
      if (existing)
        return packAdded(existing)
    }
    if (posted)
      return posted
    if (postError)
      throw postError
    await new Promise(r => setTimeout(r, 150))
  }
  if (postError)
    throw postError
  if (posted)
    return posted
  throw new Error($t('The torrent engine accepted the magnet but gave it no id.'))
}

/**
 * Import a raw .torrent file by sending its bytes to the engine. librqbit
 * accepts both magnet links and raw .torrent data as the POST body.
 */
export async function addTorrentBytes(bytes: Uint8Array) {
  const folder = downloadDir ? `&output_folder=${encodeURIComponent(downloadDir)}` : ''
  let res: Response
  const buf = new ArrayBuffer(bytes.byteLength)
  new Uint8Array(buf).set(bytes)
  try {
    res = await fetch(`${ENGINE}/torrents?overwrite=true${folder}`, {
      method: 'POST',
      body: buf,
      headers: { 'content-type': 'application/x-bittorrent' },
    })
  }
  catch {
    throw new Error($t('Torrent engine offline. Launch the native desktop or Android app to play torrents.'))
  }
  if (!res.ok)
    throw new Error($t('Torrent engine said {status}: {reason}', { status: res.status, reason: await res.text() }))
  const added = await res.json() as {
    id: number | null
    details: { name: string | null, info_hash: string, files: EngineFile[] | null }
  }
  if (added.id == null)
    throw new Error($t('The torrent engine accepted the file but gave it no id.'))
  return { ...added, id: added.id }
}

/**
 * How release names spell one episode: "S01E02", "s1e2", "S01.E02", "1x02".
 * `0*` covers both zero-padded and bare numbers, and the trailing guard keeps
 * E02 from matching E020.
 */
function episodePattern(season: number, episode: number) {
  return new RegExp(`(?:s0*${season}[\\s._-]*e0*${episode}|\\b0*${season}x0*${episode})(?!\\d)`, 'i')
}

/**
 * Which file inside the torrent to play. A season pack holds every episode and
 * only the file names say which is which, so the wanted episode is matched by
 * name first — the addon's `fileIdx` is missing on plenty of sources, and
 * falling straight through to "largest video" is what quietly plays episode 1
 * when you asked for episode 2.
 */
export function pickVideoFile(
  files: EngineFile[],
  hint: number | null,
  want?: { season?: number, episode?: number },
) {
  const videos = files
    .map((f, index) => ({ ...f, index }))
    .filter(f => VIDEO_EXT.test(f.name))
  if (!videos.length)
    return null

  if (want?.season && want?.episode) {
    const pattern = episodePattern(want.season, want.episode)
    const match = videos.find(f => pattern.test(f.name))
    if (match)
      return match.index
  }

  if (hint != null && videos.some(f => f.index === hint))
    return hint

  // A torrent already in the engine remembers what it was narrowed to, which is
  // the file someone picked last time — better than guessing again.
  const included = videos.filter(f => f.included)
  const from = included.length && included.length < videos.length ? included : videos
  return from.sort((a, b) => b.length - a.length)[0]!.index
}

/** Full path inside the torrent — a `Subs/` folder spells the episode there. */
function filePath(f: EngineFile) {
  return f.components?.length ? f.components.join('/') : f.name
}

/** The episode a file name spells out, if it spells one at all. */
function episodeIn(name: string) {
  const m = name.match(/\bs(\d{1,2})[\s._-]*e(\d{1,3})(?!\d)|\b(\d{1,2})x(\d{2})(?!\d)/i)
  return m ? { season: Number(m[1] ?? m[3]), episode: Number(m[2] ?? m[4]) } : null
}

/**
 * The first token in a release name that can only be a technical detail, which
 * is therefore where the title stops. Year, season/episode, resolution, source,
 * codec, audio — in roughly the order they actually turn up.
 *
 * Deliberately short. Every extra word is a chance to cut a real title in half,
 * and a name almost always reaches one of the first three before anything else.
 */
const DETAIL = /\b(?:(?:19|20)\d{2}|s\d{1,2}(?:[\s.,_-]*e\d{1,3})?|\d{1,2}x\d{2}|\d{3,4}p|4k|uhd|bluray|blu-ray|bdrip|brrip|dvdrip|web-?dl|web-?rip|hdtv|hdrip|remux|amzn|dsnp|atvp|x26[45]|h\.?26[45]|hevc|avc|xvid|divx|aac|ac3|eac3|ddp?\d|truehd|atmos|repack|proper|extended|uncut|imax|complete|season)\b/gi

/** What a release name says once the scene furniture is taken off it. */
export interface ReleaseName {
  /** "House.of.the.Dragon.S01.1080p…" -> "House of the Dragon". */
  title: string
  year: string
  /** 0 when the name doesn't say. */
  season: number
  episode: number
}

/**
 * Take a release name apart into something a metadata service can be searched
 * with. `House.of.the.Dragon.S01.1080p.BluRay.x265[eztv.re]` is not a title any
 * catalogue has ever heard of, and handing it to one whole is why a magnet used
 * to find no subtitles at all.
 *
 * Scene and p2p names are `Title.Separators.Then.Every.Technical.Detail`, so the
 * title is simply everything before the first detail. Two things stop that from
 * eating real titles:
 *
 * - A year later than next year is part of the name, not a release year, which
 *   is what keeps *Blade Runner 2049* whole.
 * - A detail with nothing in front of it isn't the boundary — otherwise *1917*
 *   and *2012* parse to an empty title and match everything.
 *
 * ponytail: a title whose own words are release tokens ("Alien: Covenant" is
 * fine, "The Post 2017" is fine, but "4K" or "Extended Family" would clip) is
 * left clipped. Reach for a real parser (parse-torrent-title) only if that ever
 * shows up in practice — this is 20 lines and covers everything seen so far.
 */
export function parseRelease(name: string): ReleaseName {
  const text = (name.split('/').pop() ?? '')
    // A container extension, and the tracker's tag: "[eztv.re]", "(YTS.MX)".
    .replace(/\.(?:mkv|mp4|avi|m4v|mov|ts|webm)$/i, '')
    .replace(/[._]+/g, ' ')
    .trim()

  const limit = new Date().getFullYear() + 1
  const plausible = (token: string) => !/^\d{4}$/.test(token) || Number(token) <= limit

  let cut = text.length
  for (const m of text.matchAll(DETAIL)) {
    if (m.index && plausible(m[0])) {
      cut = m.index
      break
    }
  }

  // What's left over from a title: a leading tracker tag, and the bracket a
  // year was opened with, which the cut lands in the middle of.
  const title = text.slice(0, cut)
    .replace(/[[(][^\])]*[\])]/g, ' ')
    .replace(/\s{2,}/g, ' ')
    .replace(/[\s\-–—:[(]+$/, '')
    .trim()

  // The year is the first plausible one *after* the title, so the 2049 in
  // "Blade Runner 2049 2017" is read as part of the name and the 2017 as a year.
  const year = text.slice(cut).match(/\b(?:19|20)\d{2}\b/)
  const ep = episodeIn(text)
  return {
    title,
    year: year && plausible(year[0]) ? year[0] : '',
    season: ep?.season ?? Number(text.match(/\bs(\d{1,2})\b/i)?.[1] ?? 0),
    episode: ep?.episode ?? 0,
  }
}

/**
 * The subtitle files that belong to the video being played, so they come down
 * with it. A release that ships its own is the best copy there is — already cut
 * to this exact encode, and no OpenSubtitles round trip to get it.
 *
 * The whole difficulty is a season pack, where 60 subtitle files sit beside 60
 * episodes: the episode number is the only thing tying one to the other, and it
 * is spelled out whether they're siblings (`Show.S01E07.eng.srt`) or filed under
 * `Subs/Show.S01E07/2_English.srt`. With a single video in the torrent there is
 * nothing to tell apart, and the few MB of taking every language it ships buys
 * the whole picker.
 */
export function pickSubtitleFiles(files: EngineFile[], video: number): number[] {
  const subs = files.flatMap((f, index) => SUBTITLE_EXT.test(f.name) ? [{ f, index }] : [])
  if (!subs.length)
    return []

  const name = files[video]?.name ?? ''
  if (files.filter(f => VIDEO_EXT.test(f.name)).length < 2)
    return subs.map(s => s.index)

  const ep = episodeIn(name)
  // A pack whose files carry no episode number at all: the video's own name is
  // then the shared part, which is what a `.eng.srt` beside it repeats.
  const wanted = ep
    ? (path: string) => episodePattern(ep.season, ep.episode).test(path)
    : (path: string) => path.includes(name.replace(/\.[^.]+$/, ''))

  return subs.filter(s => wanted(filePath(s.f))).map(s => s.index)
}

/**
 * Download only these files. Without it a season pack quietly pulls all 60
 * episodes down while you watch one of them.
 */
export async function limitToFiles(id: number, indexes: number[]) {
  await fetch(`${ENGINE}/torrents/${id}/update_only_files`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ only_files: indexes }),
  }).catch(() => {}) // best effort: failing here only costs disk, not playback
}

/** After first Play opened stream/0 (or the addon's fileIdx), narrow the pack. */
async function refineTorrentFiles(
  id: number,
  index: number,
  hint: number | null,
  options: { fileIndex?: number | null, season?: number, episode?: number },
) {
  const deadline = Date.now() + 180_000
  while (Date.now() < deadline) {
    const files = (await torrentDetails(id))?.files ?? []
    if (files.length) {
      const i = options.fileIndex ?? pickVideoFile(files, hint, options) ?? index
      await limitToFiles(id, [i, ...pickSubtitleFiles(files, i)])
      return
    }
    await new Promise(r => setTimeout(r, 400))
  }
}

/** One torrent with its file list — the list endpoint doesn't carry files. `id` may be the numeric id or the info hash. */
export async function torrentDetails(id: number | string): Promise<EngineTorrent | null> {
  try {
    const res = await fetch(`${ENGINE}/torrents/${id}`)
    if (!res.ok)
      return null
    const t = await res.json() as EngineTorrent
    return typeof t?.id === 'number' ? t : null
  }
  catch {
    return null
  }
}

/** Native path to a file the engine is writing. */
export function mediaFilePath(folder: string, file: EngineFile): string {
  const sep = folder.includes('\\') ? '\\' : '/'
  const rel = file.components?.length ? file.components : [file.name]
  return [folder.replace(/[\\/]+$/, ''), ...rel].join(sep)
}

/**
 * Directory the file manager should open. A file's own folder, not the
 * session root and not the `.mkv` — `xdg-open` on a video launches a player.
 */
export function containingFolder(folder: string, file?: EngineFile | null): string {
  const root = folder.replace(/[\\/]+$/, '')
  if (!file)
    return root
  const path = mediaFilePath(root, file)
  const sep = root.includes('\\') ? '\\' : '/'
  const cut = path.lastIndexOf(sep)
  if (cut <= 0)
    return root
  const parent = path.slice(0, cut)
  // `C:\file.mkv` would otherwise yield `C:`, which is not a directory.
  return /^[a-z]:$/i.test(parent) ? parent + sep : parent
}

/** `file://` with `[]` percent-encoded — some mpv builds glob a raw path and have no `--globbing` flag. */
export function mediaFileUrl(folder: string, file: EngineFile): string {
  return pathToFileUrl(mediaFilePath(folder, file))
}

/** mpv globs `[]` in a raw path. Percent-encode so a `[EZTV]` release actually opens. */
export function pathToFileUrl(path: string): string {
  if (/^[a-z]:[\\/]/i.test(path)) {
    const rest = path.replace(/\\/g, '/')
    return `file:///${rest.split('/').map(encodeURIComponent).join('/')}`
  }
  return `file://${path.split('/').map((p, i) => (i === 0 ? p : encodeURIComponent(p))).join('/')}`
}

export function streamUrl(id: number, index: number) {
  return `${ENGINE}/torrents/${id}/stream/${index}`
}

/**
 * Every byte of this file is on disk. A growing copy is preallocated to its
 * full size, so the file's length on disk is a lie — `file_progress` / `finished`
 * is what counts.
 */
export function fileComplete(
  t: { stats?: Pick<TorrentStats, 'finished' | 'file_progress'> | null },
  index: number,
  length = 0,
) {
  if (t.stats?.finished)
    return true
  const have = t.stats?.file_progress?.[index] ?? 0
  return length > 0 && have >= length
}

/**
 * What the player should open for a file the engine already holds.
 *
 * A finished copy is the disk path so mpv can seek (the engine HTTP stream is
 * opened with `force-seekable=no`, or a 100% download starts at 0:00 and the
 * bar does nothing). A growing copy stays on the stream so mpv never sees a
 * sparse preallocated file.
 */
export function heldSrc(
  t: Pick<EngineTorrent, 'id' | 'output_folder' | 'files' | 'stats'>,
  index: number,
  file?: EngineFile | null,
): string {
  const f = file ?? t.files?.[index] ?? null
  if (f && t.output_folder && fileComplete(t, index, f.length))
    return mediaFileUrl(t.output_folder, f)
  return streamUrl(t.id, index)
}

/** What the player should open: a Direct link or a finished disk path, else the engine HTTP stream. */
export function playUrl(started: Pick<Started, 'id' | 'index' | 'url'>) {
  return started.url || streamUrl(started.id, started.index)
}

/** The `{id}/stream/{index}` a stream URL names, or null for a debrid `url`. */
export function streamParts(url: string) {
  const m = /\/torrents\/(\d+)\/stream\/(\d+)/.exec(url)
  return m ? { id: Number(m[1]), index: Number(m[2]) } : null
}

/**
 * Where one file sits in the torrent's flat byte stream, and how many pieces the
 * whole thing is cut into. Enough to turn a position in a film into a piece
 * index: pieces are uniform, so the index is that byte's share of the total and
 * the piece length itself never has to be known.
 */
export interface PieceMap {
  start: number
  length: number
  total: number
  pieces: number
}

export async function pieceMap(id: number, index: number): Promise<PieceMap | null> {
  const t = await torrentDetails(id)
  const file = t?.files?.[index]
  if (!file || !t?.total_pieces)
    return null
  return {
    start: t.files!.slice(0, index).reduce((n, f) => n + f.length, 0),
    length: file.length,
    total: t.files!.reduce((n, f) => n + f.length, 0),
    pieces: t.total_pieces,
  }
}

/**
 * Which pieces are on disk, one bit each, high bit of each byte first — the same
 * bitfield peers exchange. Worth refetching: it fills in as the download runs.
 */
export async function torrentHaves(id: number): Promise<Uint8Array | null> {
  try {
    const res = await fetch(`${ENGINE}/torrents/${id}/haves`, { headers: { accept: 'application/octet-stream' } })
    return res.ok ? new Uint8Array(await res.arrayBuffer()) : null
  }
  catch {
    return null
  }
}

/**
 * Can `fraction` of the way into that file be read without asking the swarm?
 *
 * Bitrate isn't constant, so a timestamp's byte offset is an estimate, and
 * decoding one frame reads on either side of it anyway. Hence the neighbours:
 * the answer has to stay false while any of the bytes a decoder would touch are
 * still missing, or reading them puts a piece request in front of the film.
 */
export function haveAt(map: PieceMap, haves: Uint8Array, fraction: number) {
  // Uniform pieces, so a byte's index is its share of the torrent. The last
  // piece is short, which rounds one past the end — hence the clamp.
  const at = (byte: number) => Math.min(map.pieces - 1, Math.floor((byte / map.total) * map.pieces))
  const first = at(map.start)
  const last = at(map.start + map.length)
  const piece = at(map.start + Math.max(0, Math.min(1, fraction)) * map.length)
  // A neighbour outside the file is nothing to wait for: no decoder reads past
  // the file's own bytes, and another file may not even be selected.
  const has = (i: number) => i < first || i > last || !!(haves[i >> 3]! & (0x80 >> (i & 7)))
  return has(piece - 1) && has(piece) && has(piece + 1)
}

/**
 * librqbit's per-stream lookahead: the window it treats as priority pieces,
 * measured from wherever the reader is. Everything past it is downloaded in
 * the ordinary order, so it is also how much of the start we can expect to
 * arrive before anything else.
 */
const LOOKAHEAD = 32 * 1024 * 1024

/**
 * How much of the run-up to the first frame is on disk, 0 to 1.
 *
 * The engine's own percentage is the *whole torrent*, which on a two-gigabyte
 * film reads "1%" for the first half-minute and looks like nothing is
 * happening — while the pieces that actually decide when a picture appears,
 * the ones at the head of the file, are nearly all in. This counts those
 * instead, so the number on screen is the wait the viewer is actually doing.
 */
/**
 * Is the end of the file on disk?
 *
 * A matroska keeps its Cues — the seek index — in the last few hundred
 * kilobytes, and the first thing mpv does with a *seekable* stream is jump
 * there and read them. On a torrent that has not got that far yet, the read
 * stalls the open and drags librqbit's priority window to the wrong end of the
 * file, which is a player stuck at 0:00. When those pieces are already here it
 * costs nothing, and the whole seek bar can be made to work instead of only
 * the part mpv happens to be holding — so this is what decides `seekable` on
 * `player_start`.
 *
 * Two pieces, because the seek head and the cues are separate elements and the
 * second read lands a little further along than the first.
 */
export function tailBuffered(map: PieceMap, haves: Uint8Array, pieces = 2) {
  const at = (byte: number) => Math.min(map.pieces - 1, Math.floor((byte / map.total) * map.pieces))
  const last = at(map.start + map.length)
  const first = Math.max(at(map.start), last - pieces + 1)
  for (let i = first; i <= last; i++) {
    if (!(haves[i >> 3]! & (0x80 >> (i & 7))))
      return false
  }
  return true
}

export function headBuffered(map: PieceMap, haves: Uint8Array) {
  const at = (byte: number) => Math.min(map.pieces - 1, Math.floor((byte / map.total) * map.pieces))
  const first = at(map.start)
  // A file shorter than the window ends where it ends.
  const last = at(map.start + Math.min(map.length, LOOKAHEAD))
  // Prefix only: FileStream cannot send a byte until piece 0 of the file
  // is complete, so pieces later in the window do not bring the picture
  // any closer. Counting those made "17% buffered" appear while the
  // player was still waiting on the first piece.
  let have = 0
  for (let i = first; i <= last; i++) {
    if (!(haves[i >> 3]! & (0x80 >> (i & 7))))
      break
    have++
  }
  return have / (last - first + 1)
}

/** Everything the engine holds, stats included — one request per poll. */
export async function listTorrents(): Promise<EngineTorrent[]> {
  const res = await fetch(`${ENGINE}/torrents?with_stats=true`)
  if (!res.ok)
    throw new Error($t('Torrent engine said {status}.', { status: res.status }))
  const data = await res.json() as { torrents: EngineTorrent[] }
  return data.torrents
}

/** Poll until a hash shows up in the engine list — add can return before the list catches up. */
export async function waitForEngineHash(hash: string, ms = 20_000): Promise<boolean> {
  const deadline = Date.now() + ms
  while (Date.now() < deadline) {
    const list = await listTorrents().catch(() => [])
    if (list.some(t => hashesMatch(t.info_hash, hash)))
      return true
    await new Promise(r => setTimeout(r, 400))
  }
  return false
}

// --- Seeding ------------------------------------------------------------------

/**
 * Session-wide rate ceilings, bytes/s, `null` for unlimited. Applies to peer
 * traffic only — the HTTP stream mpv reads from is not rate limited — and the
 * engine forgets them on restart, so the store re-applies them.
 */
export async function setLimits(uploadBps: number | null, downloadBps: number | null = null) {
  await fetch(`${ENGINE}/torrents/limits`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ upload_bps: uploadBps, download_bps: downloadBps }),
  }).catch(() => {}) // best effort: an unset limit costs bandwidth, not playback
}

/**
 * How fast we may seed, given the best upload rate ever measured on this line.
 *
 * A line's upload capacity can't be measured without saturating it, and there's
 * no speed test here, so the only honest number is the high-water mark of what
 * we've actually managed — which is only a real measurement while nothing is
 * capping it. Hence `probing`: seeding runs unlimited for a few minutes after
 * launch (never during playback), and what that reaches becomes the estimate.
 *
 * Four fifths of it when only seeding. While something is *downloading*,
 * forty percent — a 3.3 / 2.6 MiB/s pair is the uplink full, and TCP ACKs
 * for the download sit behind that seed. The first-launch probe is
 * unlimited only when the downlink is idle; otherwise it is the thing
 * that produces that pair.
 */
export function uploadLimit(
  peakBps: number,
  watching: boolean,
  probing: boolean,
  override = 0,
  downloading = false,
) {
  // A number typed into the settings page is a decision, not an estimate: it
  // wins outright, including over the probe and the playback back-off.
  if (override > 0)
    return override
  if (probing && !watching && !downloading)
    return null
  if (watching) {
    if (peakBps <= 0)
      return 1024 * 1024
    return Math.max(512 * 1024, Math.round(peakBps * 0.4))
  }
  // Downloading still needs some seed or the swarm throttles us. 55% of
  // a measured peak, 1 MiB/s until we have one — enough reciprocity
  // without the 2.6-up / 3.3-down pair that filled the uplink.
  if (downloading) {
    if (peakBps <= 0)
      return 1024 * 1024
    return Math.max(1024 * 1024, Math.round(peakBps * 0.55))
  }
  return Math.max(256 * 1024, Math.round(peakBps * 0.8))
}

/** `forget` drops the torrent but keeps what's on disk; `delete` removes both. */
export async function torrentAction(id: number, action: 'pause' | 'start' | 'forget' | 'delete') {
  const res = await fetch(`${ENGINE}/torrents/${id}/${action}`, { method: 'POST' })
  if (res.ok)
    return
  const reason = await res.text()
  // POSTing a magnet already puts the torrent in Live. librqbit's start
  // then 400s "already live" — that is the success case, not a failure.
  if (action === 'start' && /already live/i.test(reason))
    return
  throw new Error($t('Torrent engine said {status}: {reason}', { status: res.status, reason }))
}

export interface Started {
  /** Torrent id in the engine, or -1 for a direct link, which it never sees. */
  id: number
  /** Index of the video file inside the torrent, -1 for a direct link. */
  index: number
  /** What is playing, so the caller can come straight back to this copy. '' for a link. */
  hash: string
  /** Set for a direct link: play this instead of asking the engine for a stream. */
  url: string
  /** The release we picked, or null when the caller named one itself. */
  torrent: Release | null
  /**
   * The other releases the sources answered with, best first, current one
   * included. The player's Quality menu is built from it — Direct URLs and
   * magnets alike, so torrent engine Play can switch resolution without
   * leaving the player.
   */
  alternatives?: Release[]
}

/**
 * What the engine still holds of a torrent played before, if it holds it at
 * all. `ready` means every byte of the wanted file is on disk — that copy plays
 * with no sources, no peers and no network of any kind.
 *
 * `want` is the file the caller named; without one the same guess `startTorrent`
 * makes is made here, since a magnet on its own says nothing about which file
 * inside it anyone means.
 */
async function heldCopy(hash: string, want: number | null, of?: { season?: number, episode?: number }) {
  if (!hash)
    return null
  const listed = await listTorrents().catch(() => [])
  let held = listed.find(t => hashesMatch(t.info_hash, hash)) ?? null
  // The list can lag; librqbit also answers GET /torrents/{info_hash}.
  if (!held)
    held = await torrentDetails(canonHash(hash))
  if (!held)
    return null
  const files = held.files?.length ? held.files : (await torrentDetails(held.id))?.files ?? []
  // Metadata still in flight — not a playable copy yet. Returning an empty
  // file list made Play throw "no video file" instead of waiting, and a
  // second addTorrent POST sat on "Fetching metadata" until the user left.
  if (!files.length)
    return null
  const index = want ?? pickVideoFile(files, null, of)
  const size = index == null ? 0 : files[index]?.length ?? 0
  const have = index == null ? 0 : held.stats?.file_progress?.[index] ?? 0
  return {
    id: held.id,
    hash: held.info_hash,
    files,
    index,
    folder: held.output_folder,
    // `file_progress` can lag behind `finished`; either means every byte is here.
    ready: !!size && (have >= size || !!held.stats?.finished),
  }
}

/**
 * Info hashes as 40-char hex. Magnets and the engine disagree on spelling:
 * addons often put a 32-char base32 `btih` in the magnet, librqbit lists
 * hex. A case-sensitive (or encoding-sensitive) compare then misses a copy
 * that is already on the disk and POSTs the magnet again — and librqbit
 * waits for metadata *before* it notices it already holds that hash.
 */
export function canonHash(hash: string): string {
  const h = hash.trim().toLowerCase()
  if (/^[0-9a-f]{40}$/.test(h))
    return h
  if (!/^[a-z2-7]{32}$/.test(h))
    return h
  const alphabet = 'abcdefghijklmnopqrstuvwxyz234567'
  let bits = 0
  let value = 0
  let hex = ''
  for (const c of h) {
    const i = alphabet.indexOf(c)
    if (i < 0)
      return h
    value = (value << 5) | i
    bits += 5
    if (bits >= 8) {
      bits -= 8
      hex += ((value >> bits) & 0xFF).toString(16).padStart(2, '0')
    }
  }
  return hex.length === 40 ? hex : h
}

function hashesMatch(a: string, b: string) {
  return !!a && !!b && canonHash(a) === canonHash(b)
}

/**
 * The info hash a magnet names, '' for anything that isn't one.
 */
function magnetHash(magnet: string) {
  return magnet.match(/xt=urn:btih:([^&]+)/i)?.[1] ?? ''
}

/**
 * Play a copy the engine already holds, without adding anything.
 *
 * A growing copy is the HTTP stream, never a disk path: librqbit preallocates
 * the full size, so a partial file looks complete to mpv and hangs at 0:00.
 * The stream serves the pieces that exist and waits for the rest. A finished
 * one is the path — otherwise mpv opens the unseekable engine URL, duration
 * never lands, and dragging the bar does nothing.
 */
async function playHeld(held: NonNullable<Awaited<ReturnType<typeof heldCopy>>>, torrent: Release | null, preferStream = false): Promise<Started> {
  if (held.index == null)
    throw new Error($t('That torrent holds no video file.'))
  const included = held.files.flatMap((f, i) => f.included ? [i] : [])
  const narrowed = included.length < held.files.length
  const wanted = [held.index, ...pickSubtitleFiles(held.files, held.index)]
  const missing = wanted.filter(i => !held.files[i]!.included)
  if (missing.length) {
    const only = narrowed ? [...new Set([...included, ...wanted])] : wanted
    await limitToFiles(held.id, only)
  }
  const file = held.files[held.index] ?? null
  // A Quality pick must not reopen the hanging `file://` path of a copy we
  // already have — the engine HTTP stream is what actually starts a download
  // and what mpv can buffer. Finished Play still uses the disk path.
  const url = !preferStream && held.ready && held.folder && file
    ? mediaFileUrl(held.folder, file)
    : ''
  return { id: held.id, index: held.index, hash: held.hash, url, torrent }
}

/** Release names and TMDB titles compared on the letters only. */
function slug(s: string) {
  return s.toLowerCase().replace(/[^a-z0-9]+/g, ' ').replace(/^the /, '').trim()
}

/**
 * A copy of this title the engine already holds but never filed under it — a
 * pasted magnet, or something downloaded before the title had a `cached` entry.
 * Release names lead with the title, so the name is enough to adopt it, and
 * adopting beats searching sources the user may not even have.
 *
 * Deliberately strict: the title has to end on a word boundary (or "Alien"
 * adopts "Aliens"), a known year has to agree with the one in the release (or
 * one Dune plays as the other), and a pack only counts if it really holds the
 * wanted episode — `pickVideoFile` would otherwise fall back to the largest
 * file and play episode 1.
 */
async function heldByName(name: string, year = '', season = 0, episode = 0) {
  const want = slug(name)
  // Short titles ("Up", "It") match far too much to adopt on a name alone.
  if (want.length < 4)
    return null

  for (const t of await listTorrents().catch(() => [])) {
    const other = slug(t.name ?? '')
    if (other !== want && !other.startsWith(`${want} `))
      continue

    const found = other.slice(want.length).match(/\b(19|20)\d{2}\b/)
    if (year && found && found[0] !== year)
      continue

    if (season && episode) {
      const files = (await torrentDetails(t.id))?.files ?? []
      if (!files.some(f => episodePattern(season, episode).test(f.name)))
        continue
    }
    return t.info_hash
  }
  return null
}

/**
 * Search -> pick -> add -> download only the wanted file. The player streams the
 * result; the download button just leaves it running in the background.
 */
export async function startTorrent(options: {
  /**
   * What to search the sources with, or something that fetches it. A function
   * is only called once a search is really going to happen: a copy already on
   * disk needs no id, and that id is the TMDB round trip a downloaded film
   * would otherwise be held up by.
   */
  imdbId?: string | null | (() => Promise<string | null | undefined>)
  /** Skips the source lookup entirely. */
  magnet?: string
  /**
   * Engine info hash from the Downloads page. Tried before any magnet or
   * source search — a finished copy must not sit on "Fetching metadata".
   */
  hash?: string
  /** Ditto, for a release that was a direct link rather than a torrent. */
  url?: string
  season?: number
  episode?: number
  /** Play exactly this file — the downloads page already knows which one. */
  fileIndex?: number | null
  /**
   * Title and year as TMDB spells them, read late — the same lookup `imdbId`
   * waits for is what fills it in. Only used to adopt a download the engine
   * already holds but has under no title — see `heldByName`.
   */
  named?: () => { title: string, year?: string } | null | undefined
  /**
   * The copy this title was played from last time. Tried before anything is
   * searched, and the reason a downloaded film starts instantly and offline.
   */
  cached?: { hash: string, file: number } | null
  /** Storage budget for the pick — see `diskBudget`. Ignored with `magnet`. */
  maxBytes?: number
  /**
   * Prefer releases the player can actually decode. Defaults to what this
   * device plays with, so every caller gets it right without knowing about it.
   */
  compatible?: boolean
  /**
   * Whether the search may resolve to a torrent at all. Off, only direct links
   * are considered — nothing is added to the engine and nothing lands on the
   * disk — and a title no added server can serve throws `NoServerStream`.
   * A magnet handed in by name bypasses this: that copy was chosen by hand,
   * and it plays from wherever it already sits.
   */
  allowTorrents?: boolean
  /**
   * The Download button: never resolve to a direct link. Play with the
   * engine off opens a debrid URL and keeps nothing; a download that did
   * the same would show "In downloads" with an empty disk. A magnet handed
   * in by name already takes this path.
   */
  save?: boolean
  /**
   * Race the added sources instead of waiting for every one: playback starts
   * on the first healthy answer, and slower servers stream into
   * `onAlternativesLate` afterwards.
   */
  fast?: boolean
  /** Late server answers from a `fast` search, ranked, ready to join the candidates. */
  onAlternativesLate?: (releases: Release[]) => void
  /**
   * Magnet is about to be added. The player can open the engine stream as
   * soon as this hash appears in the list, without waiting for addTorrent.
   */
  onQueued?: (info: { hash: string, index: number | null }) => void
  onStep?: (step: string) => void
  /**
   * Play may reuse a copy of this title the engine already holds, even when
   * the magnet names a different hash — that is how a downloaded film skips
   * "Fetching metadata". A Quality pick is the other hash on purpose, so it
   * passes `false` or the menu keeps playing the same file and looks blank.
   */
  adopt?: boolean
  /**
   * Open the engine HTTP stream even when the file is already on disk. Quality
   * picks use this so a tap starts that torrent instead of reopening a
   * `file://` path that is already stuck at 0:00.
   */
  preferStream?: boolean
}): Promise<Started> {
  const step = options.onStep ?? (() => {})
  const allowTorrents = options.allowTorrents ?? true
  const adopt = options.adopt ?? true
  const viaStream = !!options.preferStream || adopt === false
  let magnet = options.magnet ?? ''
  let picked: Release | null = null
  let hint: number | null = null
  let searched: Release[] = []

  const finish = (started: Started): Started => {
    if (!searched.length)
      return started
    const alternatives = serverCandidates(
      searched,
      options.maxBytes,
      options.compatible ?? !hasNativePlayer(),
      allowTorrents,
    )
    return alternatives.length ? { ...started, alternatives } : started
  }

  /**
   * A budget of nothing is not a small budget — it means the disk has no room
   * for new bytes at all, and saying so beats the two things that used to happen
   * instead. `ranked` reads a `maxBytes` of 0 as a size limit every *sized*
   * release fails, so the honest ones reported "cams, dead, or too big" while an
   * unsized one was added, downloaded, and then deleted by the next eviction
   * poll. Called only where new bytes are really about to be pulled: a copy
   * already on the disk plays with no room at all, and must keep doing so.
   */
  const needRoom = () => {
    if (options.maxBytes != null && options.maxBytes <= 0)
      throw new Error($t('Not enough free space to download. Free some room, or pick another folder under Settings → Storage.'))
  }

  // Nothing to add, nothing to fetch, nothing to keep — the link is the stream.
  if (options.url)
    return { id: -1, index: -1, hash: '', url: options.url, torrent: null }

  // Downloads Play names the torrent the engine already holds. Look it up
  // before anything else: a 100% copy must not wait on magnets or sources.
  const givenHash = (options.hash || magnetHash(magnet)).trim()
  if (givenHash) {
    const held = await heldCopy(givenHash, options.fileIndex ?? hint, options)
    if (held)
      return finish(await playHeld(held, null, viaStream))
  }

  // A magnet the caller named is a release someone chose by hand, so it beats
  // whatever is already on the disk. Asked before the id lookup below, because
  // skipping that round trip is the point: a film on the disk plays with TMDB
  // unreachable. (Torrent-only by nature — stream-only mode never gets here,
  // because a hand-named magnet is exactly the "download it for me" ask.)
  if (!magnet && allowTorrents && options.cached) {
    const { hash, file } = options.cached
    const held = await heldCopy(hash, file)
    // On disk beats searching again, finished or still growing.
    if (held)
      return finish(await playHeld(held, null, viaStream))
  }

  // An engine copy we can name without TMDB — Downloads Play puts the release
  // name in the query, and waiting on a lookup just to re-add that hash is the
  // "Fetching metadata" hang on a torrent that is already downloading.
  if (adopt && !magnet && allowTorrents) {
    const named = options.named?.()
    if (named?.title) {
      const adopted = await heldByName(named.title, named.year, options.season, options.episode)
      if (adopted) {
        const held = await heldCopy(adopted, options.fileIndex ?? hint, options)
        if (held)
          return finish(await playHeld(held, null, viaStream))
      }
    }
  }

  if (!magnet) {
    const imdbId = typeof options.imdbId === 'function' ? await options.imdbId() : options.imdbId

    // Nothing was filed under this title, but the engine may still be holding it
    // from a pasted magnet or a download the app didn't start. Read after the
    // lookup above, because that is what fills the title in. Adopting beats a
    // search, and is the only thing that works with no sources configured —
    // and, like the cached copy above, it is torrent-only: stream-only mode is
    // about not pulling bytes, so nothing already on the disk counts either.
    const named = options.named?.()
    const adopted = allowTorrents && named?.title
      ? await heldByName(named.title, named.year, options.season, options.episode)
      : null

    if (adopted) {
      magnet = magnetForHash(adopted)
    }
    else {
      if (!imdbId)
        throw new Error($t('TMDB has no IMDb id for this title, so there is nothing to look it up with.'))

      // Nothing held, nothing adopted: whatever this finds has to be fetched.
      // Unless the engine is off, in which case whatever this finds is a link
      // that streams — a full disk is no reason to refuse to play it. The real
      // add further down carries the same check for the paths that reach it.
      if (allowTorrents)
        needRoom()

      // Fast mode races the sources: playback starts on the first healthy
      // answer and slower servers flow into the candidate list as they land.
      // Sources can be slow to warm up on the first request (DNS, TLS, cold
      // caches). A generous grace on the first attempt catches the second
      // and third source answering a beat after the first; retries shrink
      // the window because the sources are warm by then.
      let found: Release[] = []
      const MAX_SEARCH_ATTEMPTS = options.fast ? 2 : 3
      for (let attempt = 1; attempt <= MAX_SEARCH_ATTEMPTS; attempt++) {
        step(attempt === 1 ? $t('Searching your sources…') : $t('Retrying sources…'))
        try {
          found = options.fast
            ? await findReleasesFast(imdbId, options.season ?? 0, options.episode ?? 0, {
                // Engine on: start on the first magnet, don't wait 2s for a
                // Direct URL. Engine off: first link plays at once — no extra
                // grace for a second server that is still resolving.
                graceMs: attempt === 1 ? (allowTorrents ? 50 : 0) : 0,
                needUrl: !allowTorrents,
                needMagnet: allowTorrents && !options.save,
                onLate: late => {
                  const more = serverCandidates(late, options.maxBytes ?? MAX_BYTES, options.compatible ?? !hasNativePlayer(), allowTorrents)
                  if (more.length)
                    options.onAlternativesLate?.(more)
                },
              })
            : await findReleases(imdbId, options.season, options.episode)
        }
        catch (searchError) {
          if (attempt < MAX_SEARCH_ATTEMPTS) {
            await new Promise(r => setTimeout(r, options.fast ? 200 : 600))
            continue
          }
          throw searchError
        }
        // Got results — stop unless the pool is empty and we have retries
        // left.  An empty pool often means the source was cold or slow on
        // the first request; a second or third shot frequently finds what
        // the first missed.
        if (found.length || attempt >= MAX_SEARCH_ATTEMPTS)
          break
        if (attempt < MAX_SEARCH_ATTEMPTS)
          await new Promise(r => setTimeout(r, options.fast ? 200 : 600))
      }
      searched = found

      // Stream-only mode narrows before ranking: a torrent release is not a
      // worse pick, it is no pick at all. A save is the opposite — a link
      // keeps nothing, so only magnets count.
      const pool = options.save
        ? found.filter(t => t.magnet)
        : allowTorrents ? found : found.filter(t => !!t.url)
      picked = options.save
        ? pickBest(pool, options.maxBytes, options.compatible ?? !hasNativePlayer(), true)
        : pickPlay(pool, options.maxBytes, options.compatible ?? !hasNativePlayer(), allowTorrents)
      if (!picked) {
        if (options.save && found.length)
          throw new Error($t('Nothing here is a download — these sources only stream this title.'))
        if (found.length && !allowTorrents) {
          // Which added servers can't serve this mode? Named on the explainer,
          // so "add one that streams" is actionable rather than abstract.
          const hosts = [...new Set(found.filter(t => !t.url).map(t => (t.via ? sourceHost(t.via) : '')))].filter(Boolean)
          const names = hosts.join(', ')
          const msg = $t('None of your added sources stream this title directly. Add a source that answers with a Direct link, or set How Play works to Torrent engine.')
            + (names ? ` ${$t('{servers} serve downloads only here.', { servers: names })}` : '')
          throw new NoServerStream(msg, hosts)
        }
        throw new Error(found.length
          ? $t('All {count} releases found were cams, dead, or too big for this device.', { count: found.length })
          : $t('Your sources have nothing for this title.'))
      }

      // Engine off, or a link with no hash: the URL is the stream. Engine on
      // with a magnet: the engine plays while the rest of the file downloads.
      if (picked.url && !options.save && !(allowTorrents && picked.magnet)) {
        return {
          id: -1,
          index: -1,
          hash: '',
          url: picked.url,
          torrent: picked,
          alternatives: serverCandidates(found, options.maxBytes, options.compatible ?? !hasNativePlayer(), allowTorrents),
        }
      }
      magnet = picked.magnet
      hint = picked.fileIdx
      // Quality pills on the player *during* the metadata wait. Waiting until
      // addTorrent returns left first Play as a spinner with no 720p/1080p/4K.
      const alts = serverCandidates(found, options.maxBytes, options.compatible ?? !hasNativePlayer(), allowTorrents)
      if (alts.length)
        options.onAlternativesLate?.(alts)
    }
  }

  // The engine may already hold whatever we ended up with: the downloads page
  // plays by magnet and never by title, and an adopted download is by definition
  // already there. Re-adding a hash it holds makes librqbit re-open a torrent it
  // is already serving — which is the "fetching metadata" wait a film that
  // finished downloading sat through with every byte of it on the disk.
  const already = await heldCopy(magnetHash(magnet), options.fileIndex ?? hint, options)
  if (already)
    return finish(await playHeld(already, picked, viaStream))

  // Sources often return a different hash than the copy already downloading
  // for this title. Adding that magnet is the "Fetching metadata" wait on a
  // film whose pieces are already on disk. A Quality pick names the other
  // hash on purpose — adopting here would keep the same picture and look
  // like the menu did nothing, or punch the player out onto a blank 0:00.
  const named = options.named?.()
  if (adopt && allowTorrents && named?.title) {
    const adopted = await heldByName(named.title, named.year, options.season, options.episode)
    if (adopted && !hashesMatch(adopted, magnetHash(magnet))) {
      const held = await heldCopy(adopted, options.fileIndex ?? hint, options)
      if (held)
        return finish(await playHeld(held, picked, viaStream))
    }
  }

  step($t('Fetching metadata from peers…'))
  const queuedHash = magnetHash(magnet)
  const queuedIndex = options.fileIndex ?? hint ?? null
  needRoom()
  if (queuedHash)
    options.onQueued?.({ hash: queuedHash, index: queuedIndex })
  let added
  try {
    added = await addTorrent(magnet)
  }
  catch (engineError) {
    // Engine offline (browser mode) — fall back to the direct URL if available.
    const directFallback = picked?.url || options.url
    if (directFallback && !options.save)
      return { id: -1, index: -1, hash: '', url: directFallback, torrent: picked }
    throw engineError
  }
  const files = added.details.files ?? []
  // First Play used to wait for the file list before handing mpv a URL. The
  // engine already had an id (Downloads showed the torrent) and the second
  // Play streamed; the first sat on "Fetching metadata from peers…". Open
  // the source's file index (or 0) the moment we have an id.
  const index = files.length
    ? (options.fileIndex ?? pickVideoFile(files, hint, options))
    : (queuedIndex ?? 0)
  if (index == null)
    throw new Error($t('That torrent holds no video file.'))

  if (!files.length) {
    void refineTorrentFiles(added.id, index, hint, options)
    return finish({
      id: added.id,
      index,
      hash: added.details.info_hash || queuedHash,
      url: '',
      torrent: picked,
    })
  }

  // Adding a magnet the engine already holds hands back its current selection,
  // so a pack you're part-way through keeps downloading what it was told to and
  // gains this file — rather than being reset to it.
  const included = files.flatMap((f, i) => f.included ? [i] : [])
  const narrowed = included.length < files.length
  // The subtitles this release ships come down with the video: a few hundred KB
  // each, and the engine only serves a file it was told to download.
  const wanted = [index, ...pickSubtitleFiles(files, index)]
  const only = narrowed ? [...new Set([...included, ...wanted])] : wanted

  // Metadata is the first honest answer about size, and for a lot of releases it
  // is the *only* one: `ranked` lets a release through when its source reported
  // no size at all (`!t.bytes`), so the budget filter never saw these. One that
  // turns out to be twice the budget used to download anyway and then be deleted
  // mid-flight by an eviction poll — the same "starts and then vanishes" this
  // whole path exists to stop, just ten minutes later. Refuse it now, while
  // nothing is on the disk and there is still something useful to say.
  //
  // Only the files we asked for count: `limitToFiles` below is what keeps a
  // season pack from pulling all sixty episodes, so the pack's own total is not
  // the number the budget is about.
  if (options.maxBytes != null && !narrowed) {
    const bytes = only.reduce((n, i) => n + (files[i]?.length ?? 0), 0)
    if (bytes > options.maxBytes) {
      // Nothing has been downloaded yet, so this reclaims the metadata and the
      // engine entry rather than any real bytes — and leaving it listed would put
      // a torrent on the Downloads page that we just told the user was too big.
      await torrentAction(added.id, 'delete').catch(() => {})
      throw new Error($t('That release needs {size} but only {free} is free. Free some room, or pick another folder under Settings → Storage.', {
        size: bytesText(bytes),
        free: bytesText(options.maxBytes),
      }))
    }
  }

  await limitToFiles(added.id, only)
  return finish({ id: added.id, index, hash: added.details.info_hash, url: '', torrent: picked })
}

export function magnetForHash(hash: string) {
  return magnetFor(hash, hash)
}

// --- Disk budget --------------------------------------------------------------
// A watched torrent is a cache, not a library: the engine keeps every byte it
// ever downloaded, and on a 64 GB TV two 4k films fill the device. So the cache
// gets a byte budget (not a torrent count — one 4k remux is thirty episodes)
// and the least recently played torrents are deleted once it's exceeded.

/** Free space that stays free whatever we're doing, so the device keeps working. */
const RESERVE_MIN = 3 * 1024 ** 3
const RESERVE_MAX = 20 * 1024 ** 3

export interface DiskSpace {
  free: number
  total: number
}

/**
 * How many bytes the torrent cache may hold. `used` is what it holds now: the
 * disk's `free` excludes that, and the cache is allowed to reuse its own space.
 * `cap` is the user's own ceiling in bytes, 0 for "whatever the disk allows".
 *
 * An unreadable disk gives no budget at all (Infinity) — never guess a limit
 * and start deleting films off the back of it.
 */
export function diskBudget(disk: DiskSpace | null, used: number, cap = 0) {
  if (!disk?.total)
    return Number.POSITIVE_INFINITY
  // Ours to spend: what is free, plus what the cache already holds and may reuse.
  const room = disk.free + used
  const sized = Math.min(Math.max(disk.total * 0.1, RESERVE_MIN), RESERVE_MAX)
  // A share of the *device* is the wrong reserve once a big disk is nearly full.
  // 10% of a 235 GB drive is the 20 GiB cap, so a drive with 16 GB free — several
  // films' worth — produced a budget of exactly 0, and a budget of 0 meant every
  // torrent holding a single byte was deleted on the next two-second poll. Hence
  // the second clamp: the reserve never takes more than half of what we actually
  // have to spend, and never drops below the floor, so a genuinely full disk
  // still yields nothing while a roomy one stops pretending it has no room.
  const reserve = Math.min(sized, Math.max(RESERVE_MIN, room * 0.5))
  return cap > 0 ? Math.min(cap, Math.max(0, room - reserve)) : Math.max(0, room - reserve)
}

/** All eviction needs of a torrent: what it is, and what it costs on disk. */
type Cached = Pick<EngineTorrent, 'id' | 'info_hash'> & { stats?: { progress_bytes: number } }

export function usedBytes(torrents: Cached[]) {
  return torrents.reduce((n, t) => n + (t.stats?.progress_bytes ?? 0), 0)
}

/**
 * How long a torrent is safe from eviction after it was last asked for.
 *
 * `keep` below is the torrent being *watched*, which is the only thing playback
 * ever marks. A press of Download marks nothing, so on a disk with a tight
 * budget the poll answered it by deleting the very torrent it had just added,
 * two seconds later — the download that "starts and then vanishes". A cache is
 * what nobody is waiting for, and someone who pressed a button ten seconds ago
 * is waiting.
 */
const EVICT_GRACE = 10 * 60_000

/**
 * Which torrents to delete to get back under budget. Oldest `touched` first
 * (when it was last played, or first seen), so tonight's episode outlives the
 * film you watched last month. `keep` is what's playing right now.
 *
 * No pinning — everything here is treated as a cache. If someone
 * wants an offline library, that's a "keep" flag on the torrent and one more
 * `filter` below.
 */
export function planEviction(
  torrents: Cached[],
  budget: number,
  keep: number | null,
  touched: Record<string, number>,
  now = Date.now(),
) {
  let used = usedBytes(torrents)
  if (used <= budget)
    return []

  const drop: number[] = []
  const oldest = [...torrents].sort((a, b) => (touched[a.info_hash] ?? 0) - (touched[b.info_hash] ?? 0))
  for (const t of oldest) {
    if (used <= budget)
      break
    if (t.id === keep)
      continue
    // Just asked for — see EVICT_GRACE.
    if (now - (touched[t.info_hash] ?? 0) < EVICT_GRACE)
      continue
    const have = t.stats?.progress_bytes ?? 0
    // Metadata fetch — nothing on disk yet, so deleting frees no room and a
    // release someone just sent to Download vanishes while older copies stay.
    if (!have)
      continue
    drop.push(t.id)
    used -= have
  }
  return drop
}

/**
 * "Only download on Wi-Fi", applied to what is running right now.
 *
 * `running` is every torrent still pulling bytes and `keep` the one being
 * watched — playback is something the user just asked for on this network, so it
 * is never held back; the toggle is about downloads nobody is waiting for.
 *
 * `held` is what earlier calls stopped, and the only thing a return to Wi-Fi
 * starts again: a torrent the *user* paused has to stay paused. It accumulates
 * because a paused torrent drops out of `running` on the very next poll.
 */
export function planNetwork(running: number[], keep: number | null, held: number[], stop: boolean) {
  if (!stop)
    return { pause: [], start: held, held: [] }

  const pause = running.filter(id => id !== keep)
  return { pause, start: [], held: [...new Set([...held, ...pause])] }
}

export function bytesText(n: number) {
  if (!n)
    return '0 B'
  const i = Math.floor(Math.log(n) / Math.log(1024))
  return `${(n / 1024 ** i).toFixed(1)} ${['B', 'KB', 'MB', 'GB', 'TB'][i]}`
}
