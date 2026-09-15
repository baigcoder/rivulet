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

// Browse must not probe streams. Each visible batch used to open four
// upstreams through the proxy; that froze the page and made Back dead.
const freePage = readFileSync(new URL('../app/pages/live-tv/free.vue', import.meta.url), 'utf8')
assert.doesNotMatch(freePage, /probeIds/, 'the Free TV grid must not probe streams while browsing')

const liveCard = readFileSync(new URL('../app/components/live-tv/LiveChannelCard.vue', import.meta.url), 'utf8')
assert.match(liveCard, /\$t\('LIVE'\)/, 'each Free TV card shows a LIVE tag')
assert.match(liveCard, /\$t\('Offline'\)/, 'and flips it to Offline when the stream is dead')
assert.match(liveCard, /absolute start-1\.5 top-1\.5/, 'the health tag sits on the artwork, top start')
assert.match(freePage, /goHub|liveTvBackPath/, 'Free TV needs an explicit way back to the hub')

const watchPage = readFileSync(new URL('../app/pages/live-tv/watch.vue', import.meta.url), 'utf8')
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
assert.match(watchPage, /proxyFreeStreamUrl\([\s\S]*\.ts/, 'm3u8 failure retries .ts through the loopback proxy')
assert.match(freePage, /saveLivePlay/, 'play must stage the stream before navigating')
assert.match(watchPage, /@retry="\(\) => void onRetry\(\)"/, 'Retry must restart the player, not only re-mint a cached proxy URL')
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
  /fromEngine\.value && started\.value && !duration\.value/,
  'unknown duration is a torrent stall, not Direct-play Buffering',
)
assert.match(
  playerSrc,
  /!fromEngine\.value && started\.value && !ended\.value && !sawPicture\.value/,
  'Loading stays up until a decoded frame — live HLS moves time-pos before the picture exists',
)
// And comes down once there has been one. Every other reading in that test is
// a reading of *now*, and on Android a pause satisfies all of them: neither
// backend reports a size for a live track and a paused clock does not move, so
// "Opening the stream…" sat over a channel that had been playing for half an
// hour. A stream that has shown a frame is not opening.
assert.match(
  playerSrc,
  /const sawPicture = ref\(false\)/,
  'and the player remembers that a frame arrived, which no reading of now can tell it',
)
assert.match(
  playerSrc,
  /opening = !fromEngine\.value && !!props\.src && \(/,
  'the native surface hides until a frame exists, including free and premium live',
)
assert.match(
  watchPage,
  /hasPicture/,
  'the free HUD must not call a black screen "playing"',
)

// --- Pause is not a connection problem ----------------------------------------
// Every reading of "there is a picture" is a reading of *now*, and on Android
// the only one left for a live channel is the clock — libVLC reports no size
// for a live track. A paused clock does not move, so within two seconds of a
// pause the page could not tell a channel the viewer stopped from one that
// never opened, and put the connecting panel over the first. It stayed up
// through the resume, which is the whole "pause, play, Reconnecting for ever"
// report.
assert.match(
  watchPage,
  /!hasPicture\.value\s+&& !playerPaused\.value/,
  'a channel the viewer paused is not still connecting',
)
// The reading itself belongs to the shared mirror — see the assertions on
// usePlayerMirror below, where the rest of it is pinned.
const premiumWatch = readFileSync(new URL('../app/pages/live-tv/premium/watch.vue', import.meta.url), 'utf8')
assert.match(
  premiumWatch,
  /if \(asBool\(p\.paused\)\)\s+premium\.setPlayer\('paused'\)\s+else if \(asBool\(p\.buffering\) \|\| !picture\)/,
  'Premium reads paused before buffering, for the same reason',
)

// --- A live stream has no timeline --------------------------------------------
// The seek buttons and the bar are hidden for live. The double-tap thirds and
// the keyboard keys were not, and libVLC answers `time-pos` on a live channel
// by tearing its output down — a tap on the side of the picture went black and
// stayed black.
assert.match(
  playerSrc,
  /function seekTo\(t: number\) \{[\s\S]{0,700}?if \(isLive\.value\)\s+return/,
  'seekTo refuses on live, at the one point every seek path goes through',
)

// --- Play on a live channel means the live edge -------------------------------
// What is behind a paused live demuxer is twenty seconds of stale cache with no
// connection behind it. Clearing `pause` plays that out and then stalls for
// good; reopening the URL is what live means.
assert.match(
  playerSrc,
  /if \(isLive\.value && !willPause\) \{\s+void goLive\(\)/,
  'resuming a live channel reopens it rather than unpausing a dead buffer',
)

// --- The stall recovery has to run where the stalls are ------------------------
// `videoWidth > 0` is false for most live channels on Android for the whole of
// playback, so the one thing that recovers a stalled channel never ran on the
// platform that stalls most.
assert.match(
  playerSrc,
  /isLive\.value && started\.value && sawPicture\.value && buffering\.value/,
  'the live recovery asks whether this stream ever had a picture, not whether it has one now',
)
assert.doesNotMatch(
  playerSrc,
  /isLive\.value && started\.value && videoWidth\.value > 0 && buffering\.value/,
  'and never goes back to the reading libVLC cannot answer',
)
assert.match(
  readFileSync(new URL('../app/components/live-tv/LivePlayerOverlay.vue', import.meta.url), 'utf8'),
  /<footer[\s\S]{0,120}data-cut/,
  'the bar that carries the connecting panel punches through the native mpv window',
)
assert.match(
  watchPage,
  /:connecting="waiting"/,
  'the live HUD must stay up until a frame exists so Back is hittable on Win32',
)
assert.match(
  readFileSync(new URL('../app/components/live-tv/LivePlayerOverlay.vue', import.meta.url), 'utf8'),
  /v-if="connecting && !error"[\s\S]{0,2500}emit\('retry'\)/,
  'Connecting… must offer Retry in the bar (Back is the header\'s) — a pointer-events-none spinner left the Windows build stuck',
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
assert.match(vlcPlayer, /length <= 0 \|\| pos < duration/, 'opening a Direct URL is a stall, not a pause')
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

// --- Channels that answer, and a proxy that doesn't break them --------------
// Measured from a home connection: a third to a half of the free list does not
// answer at all, and of the channels that did, the proxy broke some that play
// direct. The Rust halves have unit tests; these hold the wiring in place.

const rust = (rel: string) => readFileSync(new URL(`../src-tauri/src/${rel}`, import.meta.url), 'utf8')
const proxyRs = rust('iptv/proxy.rs')
assert.match(proxyRs, /match resp\.bytes\(\)\.await \{\s+Ok\(raw\) => \{\s+let body = manifest_text\(&raw\);/, 'a gzipped playlist is decoded before it is rewritten')
assert.match(proxyRs, /fn manifest_text[\s\S]*?GzDecoder/, 'by its gzip magic')
assert.match(proxyRs, /starts_with\('#'\) \{[\s\S]{0,400}?rewrite_uri_attrs\(line, base, user_agent, referer\)/, 'URI="…" inside a tag goes through the proxy too')
assert.match(proxyRs, /fn is_xtream_media_url\(url: &str\) -> bool \{\s+xtream_kind\(url\)\.is_some\(\)/, 'Xtream is the panel shape, not "/live/ somewhere in the path"')
assert.doesNotMatch(proxyRs, /path\.contains\("\/live\/"\)/, 'no substring test for /live/ is left')

const healthRs = rust('iptv/health.rs')
assert.match(healthRs, /buffer_unordered\(CONCURRENCY\)/, 'the sweep is bounded')
assert.match(healthRs, /ask the failures once\s+\/\/ more before hiding/, 'and a channel must fail twice to be hidden')
const dbRs = rust('iptv/db.rs')
assert.match(dbRs, /vec!\["COALESCE\(health, 1\) != 0"\.into\(\)\]/, 'every list leaves out what the sweep found dead, and keeps what it has not asked')
assert.match(dbRs, /ALTER TABLE iptv_channels ADD COLUMN health INTEGER/, 'an existing database gains the column')
const importer = rust('iptv/streaming_m3u.rs')
assert.equal(importer.match(/\} else if is_web_page\(&stream_url\) \{/g)?.length, 2, 'a YouTube/Twitch page is skipped on both import paths')
assert.match(m3u, /const IMPORT_REVISION: &str = "r4"/, 'and installs holding them re-import once')
const libRs = rust('lib.rs')
assert.equal(libRs.match(/iptv::health::spawn_sweep\(/g)?.length, 2, 'a sweep follows the boot import, or runs at boot when there is none')
assert.match(rust('iptv/commands.rs'), /spawn_sweep\(app\.clone\(\), true\)/, 'and follows Refresh')
assert.match(freePage, /listen<\{ done: boolean \}>\('live_health'/, 'the page re-reads the list when a sweep lands')
assert.match(freePage, /liveOfflineCount\(/, 'and says how many channels it is hiding')

// --- A guide, not a logo wall ------------------------------------------------
// Rows carry what a viewer chooses by — number, name, what is on and how far
// in — and twice as many fit a TV screen as logo tiles did.

const strip = readFileSync(new URL('../app/components/live-tv/ChannelStrip.vue', import.meta.url), 'utf8')
assert.match(strip, /h-\[72px\]/, 'a guide row is a fixed height: Free TV places rows from an estimate')
assert.match(strip, /role="button"\s+tabindex="-1"/, 'the row\'s favourite star stays out of the remote\'s path')
for (const rel of ['app/components/live-tv/LiveChannelGrid.vue', 'app/components/premium-tv/PremiumChannelGrid.vue']) {
  const s = readFileSync(new URL(`../${rel}`, import.meta.url), 'utf8')
  assert.match(s, /props\.layout === 'list' \? 1 :/, `${rel}: a guide list is one column`)
  assert.match(s, /<live-tv-channel-strip/, `${rel}: and draws guide rows`)
}
assert.match(
  readFileSync(new URL('../app/components/live-tv/LiveChannelGrid.vue', import.meta.url), 'utf8'),
  /const STRIP_ROW = 76/,
  'the Free TV estimate is the strip\'s real height, or rows overlap',
)
assert.match(freePage, /:layout="layout"/, 'Free TV follows the layout choice')

const overlaySrc = readFileSync(new URL('../app/components/live-tv/LivePlayerOverlay.vue', import.meta.url), 'utf8')
// One connecting message, and it is the bar's: which step, which attempt,
// Retry and the next channel. A second spinner over the middle of the picture
// said the same thing twice.
assert.match(overlaySrc, /connectDetail \|\| \$t\('Opening the stream…'\)/, 'connecting says which step is running')
assert.match(overlaySrc, /v-if="connecting && !error"[\s\S]{0,2500}nextEntry/, 'and offers the next channel')
// Class membership, not a substring: `border-2` is one word in the attribute
// and `border-24` is a different class. A single regex for that needs a `\b`
// after `[^"]*`, which can never require anything — so split the words instead.
const bareBorder2 = [...overlaySrc.matchAll(/class="([^"]*)"/g)]
  .map(m => (m[1] ?? '').split(/\s+/))
  .filter(words => words.includes('border-2') && !words.includes('border-solid'))
assert.deepEqual(
  bareBorder2,
  [],
  'every bordered marker says border-solid: this app sets no default border style, so a bare border-2 draws nothing',
)
assert.doesNotMatch(watchPage, /Connecting to live stream…/, 'Free TV draws no centre connecting layer')
assert.match(watchPage, /:connect-detail="connectDetail"/, 'its auto-skip attempt is said in the bar instead')
const premiumWatchSrc = readFileSync(new URL('../app/pages/live-tv/premium/watch.vue', import.meta.url), 'utf8')
assert.doesNotMatch(premiumWatchSrc, /First connect, and every reconnect/, 'nor does Premium')
assert.match(premiumWatchSrc, /:connect-detail="statusLine"/, 'Premium\'s reconnect attempt is said in the bar')
assert.match(premiumWatchSrc, /:resolving="!isVod && hudConnecting"/, 'and mpv\'s own centre spinner stands down while the bar says it')
assert.match(
  readFileSync(new URL('../app/pages/live-tv/premium/watch.vue', import.meta.url), 'utf8'),
  /:status="statusLine"/,
  'Premium\'s reconnect attempts are said in the player\'s own loading line',
)
assert.match(overlaySrc, /isProviderConnectionLimit\(props\.error\)/, 'a taken connection slot is named as such')
assert.doesNotMatch(overlaySrc, /red-600\/20|#E50914/, 'the lineup and slider use the theme primary, not a second red')
assert.doesNotMatch(playerSrc, /Starting mpv/, 'nobody watching TV needs to know what mpv is')

// --- The Android rung ceiling -------------------------------------------------
// A phone plays through libVLC with MediaCodec direct rendering off, so every
// frame is copied through a SurfaceTexture. Handed a 4K rung it never produced
// a first frame, and both Free TV and Premium sat on "Connecting" while the
// desktop played the same channel on its GPU. Three separate builders make the
// proxy URL and the cap has to be in all three, or one path silently goes back
// to 4K on the phone.
const htmlVideoSrc = readFileSync(new URL('../app/utils/htmlvideo.ts', import.meta.url), 'utf8')
const iptvSrc = readFileSync(new URL('../app/utils/iptv.ts', import.meta.url), 'utf8')
const commandsRs = rust('iptv/commands.rs')

assert.match(htmlVideoSrc, /export const SOFT_DECODE_MAX_HEIGHT = 1080/, 'the ceiling is one named constant')
assert.match(proxyRs, /pub const SOFT_DECODE_MAX_HEIGHT: u64 = 1080/, 'and Rust agrees on the number')
// One line each, not a guard-and-body pair: `\s*\n\s*` between them is two
// quantifiers that can trade the same newlines, which backtracks badly.
for (const [src, where] of [
  [htmlVideoSrc, 'playUrl caps the ladder on Android'],
  [iptvSrc, 'wrapFreeStreamUrl too — it is the builder Free TV actually plays through'],
] as const) {
  assert.match(src, /^\s*if \(hasVlcPlayer\(\)\)$/m, `${where} (the Android guard)`)
  assert.match(src, /^\s*qs \+= `&max_height=\$\{SOFT_DECODE_MAX_HEIGHT\}`$/m, where)
}
// Premium's URL is minted in Rust (`/premium-stream/:token` redirects to it),
// so the JS caps above can never reach it.
assert.match(commandsRs, /^\s*#\[cfg\(target_os = "android"\)\]$/m, 'the Rust cap is Android-only')
assert.match(
  commandsRs,
  /#\[cfg\(target_os = "android"\)\][^;]*qs\.push_str\("&max_height="\);/,
  'proxy_free_stream_url caps Premium, and only on Android',
)
// A parameter would have let a desktop caller pass one by accident; `cfg` makes
// mpv's uncapped 4K a compile-time certainty.
assert.doesNotMatch(
  readFileSync(new URL('../src-tauri/src/player_direct.rs', import.meta.url), 'utf8'),
  /max_height/,
  'the desktop mpv path never asks for a ceiling',
)
assert.match(
  proxyRs,
  /fn prefer_highest_hls_rung\(body: &str, max_height: Option<u64>\) -> String/,
  'the sorter takes the ceiling',
)
assert.match(proxyRs, /"max_height" => max_height = decoded\.parse\(\)\.ok\(\)\.filter\(\|h\| \*h > 0\)/, 'and the proxy parses it, treating garbage as no cap')

// --- Logos may not starve the API --------------------------------------------
// A browser opens about six connections per origin. The grid asks for logos by
// the hundred and each holds its connection until the upstream answers, so a
// scrolling grid used every connection the API had and `/status` could not get
// one: blank tiles on Free TV and a Premium settings spinner that never stopped
// were the same bug from two ends. Logos live on their own origin now.
const apiRs = readFileSync(new URL('../src-tauri/src/api/mod.rs', import.meta.url), 'utf8')
assert.match(apiRs, /pub const LOGO_ADDR: &str = "127\.0\.0\.1:3033"/, 'logos have an address of their own')
assert.match(apiRs, /pub const ADDR: &str = "127\.0\.0\.1:3032"/, 'and it is not the API\'s')
assert.match(apiRs, /fn build_logo_router\(\) -> Router/, 'served by a router carrying nothing else')
assert.match(apiRs, /Err\(e\) => eprintln!\("\[premium-api\] logo port unavailable/, 'and a port it cannot have costs artwork, never the API')
const premiumUtil = readFileSync(new URL('../app/utils/premiumTv.ts', import.meta.url), 'utf8')
assert.match(premiumUtil, /const LOGO_BASE = 'http:\/\/127\.0\.0\.1:3033'/, 'the page asks that origin')
assert.match(premiumUtil, /return `\$\{LOGO_BASE\}\/api\/premium-tv\/proxy\/image/, 'for every channel logo')

// The subscription card asks about the subscription. `premium.connected` means
// an account is loaded, so an active subscription with no provider yet drew
// "No active subscription" directly above a banner saying it was active.
const premiumSettings = readFileSync(new URL('../app/pages/settings/premium-tv.vue', import.meta.url), 'utf8')
assert.match(premiumSettings, /v-if="settings\.isPremium"\s/, 'the subscription card reads the subscription')

// --- A port already taken must say so ----------------------------------------
// All three local servers bind fixed loopback ports. When one cannot, the spawn
// logged to stderr — which a packaged app has nobody reading — and the pages
// then asked a server that was not there and waited. Two orphaned mpv processes
// holding 3031 and 3032 presented exactly that way: Free TV and Premium TV both
// spinning for ever, with nothing anywhere naming the cause.
const startupRs = readFileSync(new URL('../src-tauri/src/startup.rs', import.meta.url), 'utf8')
assert.match(startupRs, /pub fn record_bind_failure/, 'a bind failure is recorded, not just printed')
assert.match(startupRs, /pub fn startup_faults\(\) -> Vec<String>/, 'and the frontend can ask for it')
assert.match(startupRs, /only one usage of each socket address/, 'the Windows wording counts as a taken port')
assert.match(startupRs, /address already in use/, 'so does the unix one')

assert.match(libRs, /startup::startup_faults,/, 'the command is registered')
assert.match(libRs, /startup::record_bind_failure\(\s*"The Free TV stream proxy"/, 'Free TV reports its port')
assert.match(libRs, /startup::record_bind_failure\(\s*"Premium TV's local server"/, 'and so does Premium')
// A port is not the only way a service never starts. Premium TV opens a
// database first and only spawns its server if that worked, so a database that
// will not open takes the settings page, the catalogue and playback with it —
// and did so in silence, which is indistinguishable from a slow one.
assert.match(startupRs, /pub fn record_fault\(line: String\)/, 'a failure that is not about a port is recordable too')
assert.match(
  libRs,
  /startup::record_fault\(format!\(\s*"Premium TV could not open its database/,
  'and the premium database is the one that takes the whole feature with it',
)

const appVue = readFileSync(new URL('../app/app.vue', import.meta.url), 'utf8')
assert.match(appVue, /v-if="startupProblems\.length"/, 'and the app says so on screen')
assert.match(
  appVue,
  /\$t\('Rivulet could not start one of its local services'\)/,
  'in a sentence, rather than a spinner that names nothing',
)
// --- Free TV: a picture ends the notice --------------------------------------
// `LivePlayerOverlay` pins its chrome while `connecting` is true, so one stuck
// flag was both complaints at once: the connecting panel sat over a channel
// that was playing, and the controls never auto-hid. `autoSkipping` is the one
// that stuck — it clears when the skip counter resets, the counter reset on
// `playerPlaying` (which is `hasPicture && !paused`), and live backends get
// `paused` wrong. Same shape as the Premium reconnect guard, same fix.
const liveWatch = readFileSync(new URL('../app/pages/live-tv/watch.vue', import.meta.url), 'utf8')
assert.match(
  liveWatch,
  /const waiting = computed\(\(\) => *\r?\n *!hasPicture\.value/,
  'a picture ends the connecting notice whatever else the page believes',
)
assert.match(
  liveWatch,
  /watch\(hasPicture, picture => \{[\s\S]{0,120}autoSkips\.value = 0/,
  'and the skip counter resets on the picture, not on a pause flag',
)
assert.doesNotMatch(
  liveWatch,
  /watch\(playerPlaying, playing => \{[\s\S]{0,120}autoSkips\.value = 0/,
  'never again on playerPlaying — that is what pinned the panel open',
)
// The automatic Retry: once per channel, only where no frame ever arrived. A
// second start costs an upstream connection, so it must not be a loop.
assert.match(liveWatch, /const AUTO_RETRY_MS = 8000/, 'a channel with no picture gets one automatic retry')
assert.match(liveWatch, /if \(hasPicture\.value \|\| autoRetried \|\| overlayError\.value\)/, 'which is skipped once a picture or an error arrives')
assert.match(liveWatch, /autoRetried = true/, 'and never fires twice for the same channel')
assert.match(liveWatch, /clearAutoRetry\(\)/, 'and its timer is cleared on teardown')

// Everything on this page yields to the picture, because `LivePlayerOverlay`
// pins its chrome on `error` as well as on `connecting` — so a stale line held
// the chrome open and painted a card over a stream the viewer was watching.
assert.match(
  liveWatch,
  /\|\| \(!hasPicture\.value \? playerCatchError\.value : ''\)/,
  'a player log line is suppressed by frames on screen, not by playerPlaying',
)
assert.match(
  liveWatch,
  /if \(gainedPicture\) \{/,
  'and the last attempt\'s errors clear when the picture arrives',
)
// Acting on a stray failure while watching is a zap away from a working
// channel — both branches of the handler are destructive.
assert.match(
  liveWatch,
  /async function onPlaybackFailed\(\) \{[\s\S]{0,700}?if \(hasPicture\.value\)\r?\n {4}return/,
  'a failure with frames on screen is not acted on',
)

// --- One reading, two pages ---------------------------------------------------
// Both live pages mirror the same component into the same shape and then draw
// different chrome over it. Two copies of that reading is what produced most of
// the bugs here: `started` was dropped from the playing test on one page and
// left on the other for four releases, the stale-error rule was fixed on one
// and not the other, the failure handler learned to defer to the picture on one
// and not the other. Each reached a viewer as the same complaint.
const mirrorTs = readFileSync(new URL('../app/composables/usePlayerMirror.ts', import.meta.url), 'utf8')
assert.match(mirrorTs, /export function usePlayerMirror\(\)/, 'the shared reading exists')
assert.match(mirrorTs, /playerPlaying\.value = picture && !playerPaused\.value/, 'and it is the one that defines playing')
// A pause is read here too, and not folded into `playerPlaying`: that one is
// false for a channel the viewer stopped and for one that never opened alike,
// so neither page could tell the connecting panel which it was looking at.
assert.match(mirrorTs, /playerPaused\.value = asBool\(p\.paused\)/, 'the pause is one reading, taken where both pages read everything else')
assert.match(mirrorTs, /playerPaused,/, 'and is handed back with the rest of it')
assert.match(mirrorTs, /if \(picture \|\| playerPosition\.value > 0\.5\)/, 'and the one that suppresses a stale player error')
for (const [name, page] of [['free', liveWatch], ['premium', premiumWatchSrc]] as const) {
  assert.match(page, /usePlayerMirror\(\)/, `the ${name} page reads the player through it`)
  // Resetting the ref on a new stream is fine; re-deriving the rule is not.
  assert.doesNotMatch(
    page,
    /playerPlaying\.value = [^\n]*asBool\(p\.paused\)/,
    `and the ${name} page does not keep its own copy of the rule`,
  )
}

// eslint-disable-next-line no-console
console.info('free tv health: ok')
