// Self-check for Free TV's channel health: `bun scripts/check-free-tv.ts`.
//
// A free playlist is a list of other people's servers, so "is this channel
// alive" has no answer the app can look up — it has to be found out, and
// found out cheaply enough to do while someone scrolls. The two halves are
// the probe verdict (what counts as alive) and the skip (where to go when
// it is not), and both are pure, so both are checkable without a network.
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { audioParamsReady, createChannelHealth, liveLocked, MAX_AUTO_SKIPS, nextPlayable, pool, probeVerdict } from '../app/utils/livehealth'
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

assert.equal(audioParamsReady(null), false)
assert.equal(audioParamsReady({}), false)
assert.equal(audioParamsReady({ samplerate: 48000 }), true, 'sound is a lock-on')
assert.equal(audioParamsReady({ 'channel-count': 2 }), true)
assert.equal(liveLocked(0, false), false, 'HLS unpaused with no decoder is not live')
assert.equal(liveLocked(0, true), true, 'audio without video-params must not auto-skip')
assert.equal(liveLocked(1920, false), true)

// --- The skip ---------------------------------------------------------------

const list = [{ id: 'a' }, { id: 'b' }, { id: 'c' }, { id: 'd' }]

assert.equal(nextPlayable(list, 0, new Set()), 1, 'nothing dead: the next one')
assert.equal(nextPlayable(list, 0, new Set(['b'])), 2, 'skips the one that just failed')
assert.equal(nextPlayable(list, 0, new Set(['b', 'c'])), 3, 'and a run of them')
assert.equal(nextPlayable(list, 0, new Set(['b', 'c', 'd'])), -1, 'a dead tail is -1, not a wrap')
assert.equal(nextPlayable(list, 3, new Set()), -1, 'the end of the list is the end of the list')
assert.equal(nextPlayable(list, 3, new Set(['c']), -1), 1, 'and it walks backwards for channel-down')
assert.equal(nextPlayable(list, -1, new Set()), 0, 'from nowhere, the first')

// --- The book ----------------------------------------------------------------
// Playback writes the map; the cards only read it. A GET 403 is not a
// verdict (those CDNs refuse a fetch and still play), so the only writer
// the UI trusts is markLive / markOffline.

const book = createChannelHealth()
assert.equal(book.healthOf('a'), 'unknown', 'unseen is unknown, not live')
book.markLive('a')
assert.equal(book.healthOf('a'), 'live')
book.markOffline('a')
assert.equal(book.healthOf('a'), 'offline', 'a later failure wins')
assert.ok(book.offlineIds.value.has('a'))
assert.ok(!book.liveIds.value.has('a'), 'offline and live are exclusive')
book.markLive('a')
assert.equal(book.healthOf('a'), 'live', 'a picture this session clears offline')
book.markOffline('b')
book.reset()
assert.equal(book.healthOf('a'), 'unknown', 'disconnect / new session forgets')
assert.equal(book.healthOf('b'), 'unknown')

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
const supplemented = new Set(supplements.map(([, cc]) => cc))
for (const cc of ['PK', 'IN', 'BD', 'LK', 'NP', 'AF']) {
  assert.ok(
    supplemented.has(cc),
    `${cc} is supplemented: the curated list has no South Asia group for it`,
  )
}
assert.ok(m3u.includes('pub fn free_playlists'), 'one function hands the whole set out')
assert.ok(
  m3u.includes('pub fn free_playlist_key'),
  'and one identifies the set, so adding a playlist re-imports once',
)

// iptv-org retired the hosted XMLTV tree. The old URLs 404; a miss is an
// empty guide, not a connection error, and the 25MB replacement is not
// fetched on every boot.
const epg = readFileSync(new URL('../src-tauri/src/iptv/epg.rs', import.meta.url), 'utf8')
const libRs = readFileSync(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8')
assert.doesNotMatch(epg, /api\/epg\/channels\.json/, 'iptv-org retired epg/channels.json')
assert.doesNotMatch(epg, /api\/epg\/guides\//, 'and the per-channel XMLTV files')
assert.match(epg, /api\/guides\.json/, 'guides.json is the replacement map')
assert.doesNotMatch(
  libRs,
  /epg::fetch_channel_mapping/,
  'do not download 25MB of guides.json on boot',
)

// Browse must not probe streams. Each visible batch used to open four
// upstreams through the proxy; that froze the page and made Back dead.
const freePage = readFileSync(new URL('../app/pages/live-tv/free.vue', import.meta.url), 'utf8')
assert.doesNotMatch(freePage, /probeIds/, 'the Free TV grid must not probe streams while browsing')

const liveCard = readFileSync(new URL('../app/components/live-tv/LiveChannelCard.vue', import.meta.url), 'utf8')
assert.match(liveCard, /\$t\('LIVE'\)/, 'each Free TV card shows a LIVE tag')
assert.match(liveCard, /\$t\('Offline'\)/, 'and flips it to Offline when the stream is dead')
assert.match(liveCard, /absolute start-1\.5 top-1\.5/, 'the health tag sits on the artwork, top start')
assert.match(liveCard, /live-tv-live-status-badge/, 'LIVE / Offline is one chip, shared with Premium')
const liveBadge = readFileSync(new URL('../app/components/live-tv/LiveStatusBadge.vue', import.meta.url), 'utf8')
assert.match(liveBadge, /health === 'offline'/, 'the chip reads the session book, not a probe')
assert.match(liveBadge, /\$t\('LIVE'\)/)
assert.match(liveBadge, /\$t\('Offline'\)/)
assert.match(freePage, /goHub|liveTvBackPath/, 'Free TV needs an explicit way back to the hub')

const watchPage = readFileSync(new URL('../app/pages/live-tv/watch.vue', import.meta.url), 'utf8')
const iptvSrc = readFileSync(new URL('../app/utils/iptv.ts', import.meta.url), 'utf8')
assert.match(iptvSrc, /export function wrapFreeStreamUrl/, 'Play wraps in JS, not via IPC')
assert.match(iptvSrc, /127\.0\.0\.1:3031\/stream/, 'the wrap is the existing loopback proxy')
assert.ok(iptvSrc.includes('127\\.0\\.0\\.1:3031'), 'already-wrapped URLs must not nest')
assert.match(iptvSrc, /Promise\.resolve\(wrapFreeStreamUrl/, 'the old async name is a wrap, not invoke')
assert.doesNotMatch(watchPage, /\bAspectMode\b/, 'the free player must not auto-import AspectMode — that crashed setup')
assert.match(watchPage, /from '~\/utils\/aspectRatio'/, 'aspect helpers are imported, not auto-injected')
assert.match(watchPage, /:aspect="aspectRatio"/, 'Fit/Center/Stretch is a player prop, not a CSS-only guess')
assert.match(watchPage, /:fullscreen="isFullscreen"/, 'live fullscreen is the same player-mode the film uses')
assert.match(watchPage, /setAndroidPlayerMode/, 'Android live fullscreen hides system bars through MainActivity')
assert.doesNotMatch(watchPage, /requestFullscreen/, 'the WebView has no Fullscreen API — do not call it')
assert.doesNotMatch(watchPage, /function flag\b/, 'local flag() collides with utils/flag and crashes setup')
assert.match(watchPage, /readLivePlay/, 'the player must recover the staged stream if the zap list is gone')
assert.match(watchPage, /@go-live=/, 'resume after pause must offer a jump back to the live edge')
assert.doesNotMatch(watchPage, /playNow\(rawUrl/, 'the free player must never hand mpv a raw upstream URL')
assert.match(watchPage, /wrapFreeStreamUrl/, 'Play wraps the M3U URL in-process, not via IPC')
assert.match(watchPage, /playStagedNow/, 'a staged stream must start mpv on this tick')
assert.match(watchPage, /wrapChannel\(\s*targetAlt/, 'm3u8 failure retries .ts through the loopback wrap')
assert.doesNotMatch(watchPage, /logo: ch\.logoUrl/, 'artwork URLs must not go in the query string')
assert.doesNotMatch(watchPage, /iptvProxyHealth/, 'a health ping that always returns true only delayed Play')
assert.doesNotMatch(
  freePage,
  /path: localePath\('\/live-tv\/watch'\),\s*query: \{[^}]*logo:/,
  'browse must not put artwork URLs in the player query',
)
assert.match(freePage, /saveLivePlay/, 'play must stage the stream before navigating')
assert.match(watchPage, /@retry="\(\) => void onRetry\(\)"/, 'Retry must restart the player, not only re-mint a cached proxy URL')
assert.match(
  watchPage,
  /if \(waiting\.value \|\| overlayError\.value\)/,
  'arrows on Connecting / Playback Error must walk Retry, not zap the next channel',
)
assert.match(watchPage, /@refresh="\(\) => void onRefresh\(\)"/, 'Refresh must be wired — Retry alone walked to the next channel')
assert.match(watchPage, /holdChannel/, 'Retry must keep this channel, not spend the auto-skip walk')
assert.match(
  watchPage,
  /function autoSkip\(\)[\s\S]*if \(holdChannel\.value\)\s*return false/,
  'a manual Retry must not auto-skip away from the channel the viewer asked to keep',
)
assert.match(
  watchPage,
  /:resolving="waiting"/,
  'the player must not draw a second spinner while the page owns Connecting…',
)
assert.doesNotMatch(
  watchPage,
  /:status="statusLine"/,
  'the skip sentence is the page overlay, not a second line inside Buffering',
)

const playerSrc = readFileSync(new URL('../app/components/MpvPlayer.vue', import.meta.url), 'utf8')
assert.match(
  playerSrc,
  /if \(!fromEngine\.value\) \{/,
  'Direct HTTP skips the HTTP probe — mpv starts instead of waiting on fetch',
)
assert.doesNotMatch(
  playerSrc,
  /waitForStream\(props\.src/,
  'engine must not GET the stream before mpv — that FileStream starves the first pieces',
)
assert.match(
  playerSrc,
  /errorMsg\.value \? friendlyPlaybackError/,
  'an empty player error is not the generic overlay sentence',
)
assert.match(
  playerSrc,
  /if \(isLive\.value\)\s*return ''/,
  'live mode must not paint a centre overlay — the page overlay owns connecting and errors',
)
assert.match(
  playerSrc,
  /fromEngine\.value \|\| fromDisk\.value/,
  'unknown duration is a torrent stall, not Direct-play Buffering',
)
assert.match(
  playerSrc,
  /!fromEngine\.value && started\.value && !ended\.value && videoWidth\.value === 0/,
  'Loading stays up until a decoded frame — live HLS moves time-pos before the picture exists',
)
assert.match(
  playerSrc,
  /opening = !!props\.src && \(/,
  'the native surface hides until a frame exists — torrents included, or 0:00 covers Buffering',
)
assert.match(
  playerSrc,
  /isLive\.value && !!props\.resolving && awaitingFrame/,
  'live Connecting keeps mpv unmapped so the overlay is not a black --wid window',
)
assert.match(
  playerSrc,
  /'audio-params'/,
  'the player must poll audio-params — live often has sound before a frame',
)
assert.match(
  playerSrc,
  /videoWidth\.value === 0 && !hasAudio/,
  'decoded audio maps the window and stops the skip timer',
)
assert.match(
  playerSrc,
  /isLive\.value && \(videoWidth\.value > 0 \|\| hasAudio\.value\)/,
  'a live stream that already has sound must not emit failed',
)
assert.match(
  playerSrc,
  /native && !isLive && \(centre === 'loading'/,
  'live Connecting must not punch the whole window — that hides the HUD under a black hole',
)
assert.doesNotMatch(
  playerSrc,
  /isLive\.value \|\| \(typeof p\['time-pos'\]/,
  'live must not fake 1280p — HLS is unpaused with no frame',
)
assert.match(
  watchPage,
  /hasPicture\.value = videoW > 0/,
  'the free HUD must not call a black screen "playing"',
)
assert.match(
  watchPage,
  /liveLocked/,
  'sound or a frame stops Connecting — audio-only lock-on must not zap next',
)
assert.match(
  watchPage,
  /if \(locked\.value\)\s*return/,
  'a playing channel must not enter onPlaybackFailed',
)
assert.match(
  watchPage,
  /!locked\.value && !overlayError/,
  'the connect timer must not fire once sound or a frame exists',
)
assert.match(
  watchPage,
  /armConnectTimer/,
  'a 404 loop that never kills mpv must still leave Connecting and then skip',
)
const overlaySrc = readFileSync(new URL('../app/components/live-tv/LivePlayerOverlay.vue', import.meta.url), 'utf8')
assert.match(
  overlaySrc,
  /v-if="busy && !error"[\s\S]*?data-cut[\s\S]*?bg-\[#0F1117\]/,
  'Connecting is a small opaque card, not a full-screen black sheet',
)
assert.doesNotMatch(
  overlaySrc,
  /v-if="busy && !error"[\s\S]{0,80}data-cut/,
  'data-cut on the full-screen wrapper punches the whole player away',
)
assert.match(
  overlaySrc,
  /busy && !error[\s\S]*proxyLogo\(channelLogo\)/,
  'Connecting names the channel with its mark, not only a spinner',
)
assert.match(
  overlaySrc,
  /busy && !error[\s\S]*\$t\('Next channel'\)[\s\S]*\$t\('Retry'\)[\s\S]*\$t\('Refresh'\)[\s\S]*\$t\('Back'\)/,
  'Connecting must offer Skip, Retry, Refresh and Back — a full-screen spinner with no actions trapped the viewer',
)
assert.match(
  overlaySrc,
  /relative flex-1[\s\S]*v-if="busy && !error"[\s\S]*<!-- QUICKZAP/,
  'Connecting sits in the middle band so Back and transport stay on screen',
)
assert.match(
  overlaySrc,
  /<header[\s\S]*?class="[^"]*relative z-30/,
  'live Back stays above the connecting card',
)
assert.match(
  overlaySrc,
  /<footer[\s\S]*?class="[^"]*relative z-30/,
  'live transport stays above the connecting card',
)
assert.match(
  overlaySrc,
  /v-if="error"[\s\S]*channelName/,
  'Playback Error names the channel that failed',
)
assert.match(
  overlaySrc,
  /canSkipChannel[\s\S]*\$t\('Next channel'\)/,
  'Playback Error Next is any other channel, not only a later index',
)
assert.match(
  overlaySrc,
  /ref="retryBtn"/,
  'Retry takes focus when playback fails so a remote is not stuck on Back',
)
assert.match(
  overlaySrc,
  /v-if="error"[\s\S]*\$t\('Retry'\)[\s\S]*\$t\('Refresh'\)[\s\S]*\$t\('Back'\)/,
  'Playback Error offers Retry, Refresh and Back as full-width actions',
)
assert.match(
  overlaySrc,
  /emit\('refresh'\)/,
  'Refresh is its own action, not a second label on Retry',
)
assert.match(
  overlaySrc,
  /:inert="centreModal"/,
  'Connecting and Playback Error keep the d-pad on the card, not Hide or transport',
)
assert.match(
  overlaySrc,
  /min-height: 2\.5rem/,
  'error actions are TV-sized, not compact wrap chips',
)
assert.match(
  overlaySrc,
  /offlineIds/,
  'the zap list must know which channels this session already gave up on',
)
assert.match(
  overlaySrc,
  /health === 'offline'/,
  'and tag those rows Offline so the lineup matches the grid',
)
assert.match(
  watchPage,
  /watch\(locked[\s\S]*liveTv\.markLive/,
  'LIVE is a decoded frame or decoded audio, not an HLS clock that ticks on a black window',
)
assert.match(
  watchPage,
  /:offline-ids="liveTv.offlineIds"/,
  'the overlay reads the same book the grid does',
)
const failFn = watchPage.slice(
  watchPage.indexOf('async function onPlaybackFailed'),
  watchPage.indexOf('async function onRetry'),
)
assert.ok(
  failFn.includes('attemptedFallback') && failFn.indexOf('attemptedFallback') < failFn.indexOf('markOffline'),
  'a .ts/.m3u8 swap must run before the channel is tagged Offline',
)
assert.match(
  watchPage,
  /function skipChannel/,
  'a dead channel from Connecting wraps the zap list instead of no-op on the last item',
)
assert.doesNotMatch(
  watchPage,
  /v-if="waiting"/,
  'Connecting… is the overlay once — a second page spinner stacked the same sentence twice',
)
assert.match(
  playerSrc,
  /cache-buffering-state/,
  'the player polls stream fill so the loader can show a percent',
)
assert.match(
  playerSrc,
  /loadPercent/,
  'Buffering shows a streaming percent, not only a spinner',
)
assert.match(
  playerSrc,
  /status && !isLive/,
  'Buffering must not append the live skip sentence',
)

const vlcPlayer = readFileSync(new URL('../src-tauri/gen/android/app/src/main/java/io/github/rivulet/rivulet/VlcPlayer.kt', import.meta.url), 'utf8')
assert.match(vlcPlayer, /fun applyVideoScale/, 'Fit/Center/Stretch must drive libVLC setScale, not setVideoScale alone')
assert.match(vlcPlayer, /SURFACE_FIT_SCREEN/, 'Center is crop-to-fill')
assert.match(vlcPlayer, /setAspectRatio/, 'Stretch forces the picture to the view')
assert.match(vlcPlayer, /http-user-agent=Mozilla/, 'libVLC must not hit debrid as Lavf')
assert.match(vlcPlayer, /else \(pos < duration\)/, 'opening a Direct URL is a stall, not a pause')
assert.match(vlcPlayer, /video-params/, 'first frame is what dismisses Loading on Android')

const androidMain = readFileSync(new URL('../src-tauri/gen/android/app/src/main/java/io/github/rivulet/rivulet/MainActivity.kt', import.meta.url), 'utf8')
assert.match(androidMain, /fun applyPlayerMode/, 'player mode is re-applied when Android restores the bars')
assert.match(androidMain, /onWindowFocusChanged/, 'MIUI shows the bars again on focus')
assert.match(androidMain, /FLAG_FULLSCREEN/, 'legacy fullscreen flag for skins that ignore WindowInsetsController')

const categoryPage = readFileSync(new URL('../app/pages/live-tv/free/category/[category].vue', import.meta.url), 'utf8')
assert.doesNotMatch(categoryPage, /LiveCategoryPage/, 'category deep-links stay on the unified Free TV shell')
assert.match(freePage, /live-tv-live-browse-header/, 'Free TV uses the shared browse header')
assert.doesNotMatch(freePage, /v-text-field/, 'Free TV must not mount a raw text field in the header')
assert.doesNotMatch(freePage, /max-width="320"/, 'the category sheet is not a 320px desktop rail on a phone')
assert.match(freePage, /:fullscreen="!smAndUp"/, 'phones get a full-width category sheet')

const hubPage = readFileSync(new URL('../app/pages/live-tv/index.vue', import.meta.url), 'utf8')
assert.match(hubPage, /list-none/, 'hub tags are chips, not a bulleted list')
assert.match(hubPage, /text-title-large/, 'hub titles fit a phone width without clipping')

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
  friendlyPlaybackError('[ffmpeg] tcp: Failed to resolve hostname dead.example: Name or service not known', 'live'),
  /offline|could not be reached/i,
  'DNS failures become a viewer sentence, not a log tail',
)
assert.doesNotMatch(
  friendlyPlaybackError('[ffmpeg] tcp: Failed to resolve hostname dead.example: Name or service not known', 'vod'),
  /channel/i,
  'a film must not say try another channel',
)
assert.doesNotMatch(
  friendlyPlaybackError('[stream] Failed to open https://example/playlist.m3u8', 'live'),
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
