// Self-check for Free TV's channel health: `bun scripts/check-free-tv.ts`.
//
// A free playlist is a list of other people's servers, so "is this channel
// alive" has no answer the app can look up — it has to be found out, and
// found out cheaply enough to do while someone scrolls. The two halves are
// the probe verdict (what counts as alive) and the skip (where to go when
// it is not), and both are pure, so both are checkable without a network.
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { MAX_AUTO_SKIPS, nextPlayable, pool, probeVerdict } from '../app/utils/livehealth'
import { liveTvBackPath, liveTvFrom, readLivePlay, saveLivePlay } from '../app/utils/liveNav'
import { connectionLimitMessage, fmtHudTime, friendlyPlaybackError, isProviderConnectionLimit, isProviderVodSlateDuration } from '../app/utils/playbackError'
import '../scripts/i18n-stub.ts'

// --- The verdict ------------------------------------------------------------
// The proxy passes the upstream status through, so this reads a real one.

assert.equal(probeVerdict(200), 'live', 'a served manifest is alive')
assert.equal(probeVerdict(206), 'live', 'so is a partial one — a live .ts is ranged')
assert.equal(probeVerdict(302), 'live', 'a redirect is a working CDN, not a dead channel')
assert.equal(probeVerdict(401), 'offline', 'an authenticated stream we cannot open is offline to us')
assert.equal(probeVerdict(403), 'offline', 'geoblocked is offline here')
assert.equal(probeVerdict(404), 'offline', 'gone is gone')
assert.equal(probeVerdict(502), 'offline', 'the proxy answers 502 when the upstream refuses the connection')

// --- The skip ---------------------------------------------------------------

const list = [{ id: 'a' }, { id: 'b' }, { id: 'c' }, { id: 'd' }]

assert.equal(nextPlayable(list, 0, new Set()), 1, 'nothing dead: the next one')
assert.equal(nextPlayable(list, 0, new Set(['b'])), 2, 'skips the one that just failed')
assert.equal(nextPlayable(list, 0, new Set(['b', 'c'])), 3, 'and a run of them')
assert.equal(nextPlayable(list, 0, new Set(['b', 'c', 'd'])), -1, 'a dead tail is -1, not a wrap')
assert.equal(nextPlayable(list, 3, new Set()), -1, 'the end of the list is the end of the list')
assert.equal(nextPlayable(list, 3, new Set(['c']), -1), 1, 'and it walks backwards for channel-down')
assert.equal(nextPlayable(list, -1, new Set()), 0, 'from nowhere, the first')

// The bound is the point: a whole dead category must not flash the player
// through the entire list and land somewhere the viewer never chose.
assert.ok(MAX_AUTO_SKIPS > 0 && MAX_AUTO_SKIPS <= 10, 'the auto-skip is bounded and small')

// --- The pool ---------------------------------------------------------------
// Probing 60 visible cards at once is 60 sockets and a stalled UI; the pool
// is what keeps it to a handful, so its concurrency has to actually hold.

const order: number[] = []
let inFlight = 0
let peak = 0
await pool([1, 2, 3, 4, 5, 6, 7, 8, 9], 3, async n => {
  inFlight++
  peak = Math.max(peak, inFlight)
  await new Promise(r => setTimeout(r, n % 3 === 0 ? 5 : 1))
  order.push(n)
  inFlight--
})
assert.equal(peak, 3, 'never more than the limit in flight')
assert.equal(order.length, 9, 'and every item still runs')

// --- The playlists ----------------------------------------------------------
// The curated worldwide list plus per-country supplements, all bundled, all
// public. No credentials, no plain HTTP, and no second URL left behind to rot:
// the array that was here before had three entries and one caller, so two of
// them were never read and nobody knew.

const m3u = readFileSync(new URL('../src-tauri/src/iptv/m3u.rs', import.meta.url), 'utf8')
const urls = [...m3u.matchAll(/"(https?:\/\/[^"]+)"/g)].map(m => m[1]!)
const playlists = urls.filter(u => /\.m3u8?(?:$|\?)/.test(u))

assert.ok(playlists.length >= 1, 'at least the curated playlist is bundled')
assert.ok(
  playlists.some(u => u.includes('Free-TV/IPTV')),
  `the curated worldwide playlist is the Free-TV list, not ${playlists.join(', ')}`,
)
for (const url of playlists) {
  // A playlist URL is compiled into the binary and shipped: it may not be a
  // credential, and it may not be interceptable.
  assert.ok(url.startsWith('https://'), `${url} must be https`)
  assert.ok(!/username=|password=|[?&]token=/i.test(url), `${url} carries a credential`)
}

// Each supplement pairs a country code with a URL, and the importer applies
// that code as its last fallback — a per-country playlist writes the country
// on no line, so without the pair every channel in it lands country-less.
const supplements = [...m3u.matchAll(/\("([A-Z]{2})",\s*"(https:\/\/[^"]+)"\)/g)]
assert.ok(supplements.length >= 1, 'at least one per-country supplement')
assert.ok(
  supplements.some(([, cc]) => cc === 'PK'),
  'Pakistan is supplemented: the curated list has no Pakistan group at all',
)
assert.ok(m3u.includes('pub fn free_playlists'), 'one function hands the whole set out')
assert.ok(
  m3u.includes('pub fn free_playlist_key'),
  'and one identifies the set, so adding a playlist re-imports once',
)

// Browse must not probe streams. Each visible batch used to open four
// upstreams through the proxy; that froze the page and made Back dead.
const freePage = readFileSync(new URL('../app/pages/live-tv/free.vue', import.meta.url), 'utf8')
assert.doesNotMatch(freePage, /probeIds/, 'the Free TV grid must not probe streams while browsing')
assert.match(freePage, /goHub|liveTvBackPath/, 'Free TV needs an explicit way back to the hub')

const watchPage = readFileSync(new URL('../app/pages/live-tv/watch.vue', import.meta.url), 'utf8')
assert.doesNotMatch(watchPage, /\bAspectMode\b/, 'the free player must not auto-import AspectMode — that crashed setup')
assert.match(watchPage, /from '~\/utils\/aspectRatio'/, 'aspect helpers are imported, not auto-injected')
assert.doesNotMatch(watchPage, /function flag\b/, 'local flag() collides with utils/flag and crashes setup')
assert.match(watchPage, /readLivePlay/, 'the player must recover the staged stream if the zap list is gone')
assert.match(watchPage, /@go-live=/, 'resume after pause must offer a jump back to the live edge')
assert.doesNotMatch(watchPage, /playNow\(rawUrl/, 'the free player must never hand mpv a raw upstream URL')
assert.match(watchPage, /proxyFreeStreamUrl\([\s\S]*\.ts/, 'm3u8 failure retries .ts through the loopback proxy')
assert.match(freePage, /saveLivePlay/, 'play must stage the stream before navigating')
assert.match(watchPage, /@retry="\(\) => void onRetry\(\)"/, 'Retry must restart the player, not only re-mint a cached proxy URL')
assert.match(
  watchPage,
  /:resolving="resolving \|\| autoSkipping"/,
  'the player must not draw Buffering while the skip notice is on screen',
)
assert.doesNotMatch(
  watchPage,
  /:status="statusLine"/,
  'the skip sentence is the page overlay, not a second line inside Buffering',
)

const playerSrc = readFileSync(new URL('../app/components/MpvPlayer.vue', import.meta.url), 'utf8')
assert.match(
  playerSrc,
  /fromIptv\.value \|\| fromProxy\.value/,
  'proxied live streams skip the HTTP probe — a live TS has no end',
)
assert.match(
  playerSrc,
  /res\.body\?\.cancel/,
  'if the probe does run, it must not read a live body into an arrayBuffer',
)
assert.match(
  playerSrc,
  /errorMsg\.value \? friendlyPlaybackError/,
  'an empty player error is not the generic overlay sentence',
)
assert.match(
  playerSrc,
  /isLive\.value && props\.resolving/,
  'live mode must not paint a centre overlay while the page owns the notice',
)
assert.match(
  playerSrc,
  /status && !isLive/,
  'Buffering must not append the live skip sentence',
)

const categoryPage = readFileSync(new URL('../app/pages/live-tv/free/category/[category].vue', import.meta.url), 'utf8')
assert.doesNotMatch(categoryPage, /LiveCategoryPage/, 'category deep-links stay on the unified Free TV shell')
assert.match(freePage, /live-tv-live-browse-header/, 'Free TV uses the shared browse header')
assert.doesNotMatch(freePage, /v-text-field/, 'Free TV must not mount a raw text field in the header')

for (const rel of [
  'app/components/live-tv/LiveChannelCard.vue',
  'app/components/live-tv/LiveBrowseHeader.vue',
  'app/pages/live-tv/index.vue',
  'app/pages/live-tv/free.vue',
  'app/components/live-tv/LiveGuideProgram.vue',
]) {
  const s = readFileSync(new URL(`../${rel}`, import.meta.url), 'utf8')
  const template = s.slice(s.indexOf('<template>'))
  for (const chunk of template.split(/(?=<[a-z])/i)) {
    if (!chunk.includes('hover:'))
      continue
    assert.ok(
      chunk.includes('focus-visible:') || chunk.includes('focus:'),
      `${rel}: an element with hover: styling has no focus twin`,
    )
  }
}

const liveTvStore = readFileSync(new URL('../app/stores/liveTv.ts', import.meta.url), 'utf8')
const clearRecentFn = liveTvStore.slice(liveTvStore.indexOf('async function clearRecent'), liveTvStore.indexOf('async function loadEpg'))
assert.match(clearRecentFn, /recentPreviews:\s*\[\]/, 'Clear recently watched must empty the list the grid reads')
assert.ok(
  clearRecentFn.indexOf('recentPreviews') < clearRecentFn.indexOf('liveClearRecent'),
  'the page must empty before waiting on IPC, or a slow command leaves the cards up',
)

assert.equal(liveTvBackPath('/live-tv/free'), '/live-tv', 'Free TV browse goes to the hub')
assert.equal(liveTvBackPath('/live-tv'), '/', 'the hub goes home, not into a stack of live-tv pages')
assert.equal(liveTvBackPath('/live-tv/'), '/', 'trailing slash on the hub is still home')
assert.equal(liveTvBackPath('/live-tv/watch'), '/live-tv/free', 'the free player goes to Free TV, not history')
assert.equal(liveTvBackPath('/live-tv/premium/watch'), '/live-tv/premium', 'the premium player goes to Premium TV')

const appBar = readFileSync(new URL('../app/components/AppBar.vue', import.meta.url), 'utf8')
assert.match(appBar, /if \(isLiveTv\.value\) \{/, 'the toolbar Back arrow uses the live-tv ladder on every live-tv route')
assert.doesNotMatch(
  appBar,
  /isLiveTv\.value && !/,
  'the hub must not fall through to router.back() — that never reaches Home',
)
assert.equal(liveTvFrom('/live-tv/watch?id=1', '/live-tv/free'), '/live-tv/free', 'a player URL is never a from= target')
assert.equal(liveTvFrom('/live-tv/free', '/live-tv'), '/live-tv/free', 'a browse path is kept')

// jsdom-less: sessionStorage exists in bun.
saveLivePlay({
  id: 'a',
  title: 'A',
  logo: '',
  sourceId: 'free:iptv-org',
  streamUrl: 'http://example/a.m3u8',
  zapList: [{ id: 'a', name: 'A', streamUrl: 'http://example/a.m3u8' }],
})
assert.equal(readLivePlay()?.id, 'a', 'staged play survives a store reset')

assert.match(
  friendlyPlaybackError('[ffmpeg] tcp: Failed to resolve hostname dead.example: Name or service not known'),
  /offline|could not be reached/i,
  'DNS failures become a viewer sentence, not a log tail',
)
assert.doesNotMatch(
  friendlyPlaybackError('[stream] Failed to open https://example/playlist.m3u8'),
  /ffmpeg|\[stream\]/,
  'decoder noise is never echoed back',
)
assert.equal(friendlyPlaybackError(''), friendlyPlaybackError(undefined), 'empty input gets one default')
assert.ok(isProviderConnectionLimit('==== Max Connection Limit Reached ===='), 'panel error slates are recognised')
assert.ok(isProviderConnectionLimit('#Mutiple Login Logs'), 'multiple-login slates are recognised')
assert.ok(isProviderVodSlateDuration(37), 'a 37-second vod file is a panel clip')
assert.ok(!isProviderVodSlateDuration(3600), 'a full-length film is not a slate')
assert.match(
  friendlyPlaybackError('==== Max Connection Limit Reached ===='),
  /connection limit/i,
  'connection-limit slates become an account sentence',
)
assert.match(connectionLimitMessage(2, 2), /2.*2/, 'active and max slots are named when known')
assert.equal(fmtHudTime(125), '2:05', 'HUD clock formats minutes')
assert.equal(fmtHudTime(3661), '1:01:01', 'HUD clock formats hours')

console.info('free tv health: ok')
