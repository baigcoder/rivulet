// Self-check for the Premium TV player and its front end:
// `bun scripts/check-premium-player.ts`.
//
// Two kinds of check, and the split is deliberate. The reconnect
// schedule is *behaviour* — it is imported and called, because "bounded
// with backoff" is a claim about numbers and a grep cannot check
// numbers. Everything else is *shape*: which file owns what. Those are
// text assertions because the alternative is mounting Vue, and the
// mistakes they catch (a second player, a provider URL in a component, a
// list that renders five thousand nodes) are all visible in the source.
//
// What this pins, and why each one was a bug once:
// - One player. `MpvPlayer.vue` plus the `<video>` shim answer the same
//   mpv protocol; a second implementation inside `premium-tv/` is how
//   HLS, reconnect and volume ended up existing twice.
// - The UI never builds a stream URL. No `player_api.php`, no
//   `/live/user/pass/id.ts`, no credentials — the only playable URL is
//   the loopback redirector's.
// - The list is virtualized. A 5,000-channel provider is the normal
//   case, not the stress test.
// - The guide degrades to nothing rather than to an empty box.
// - Every `hover:` has a `focus-visible:` twin on the same element,
//   because the remote has no pointer.

import assert from 'node:assert'
import { existsSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { MAX_RECONNECT_ATTEMPTS, reconnectDelayMs } from '../app/stores/premiumTv'
import { categoryLabel, isBundleCategory, parseCategoryName } from '../app/utils/categoryLabel'
import './i18n-stub'

const ROOT = fileURLToPath(new URL('..', import.meta.url))

const STORE = `${ROOT}app/stores/premiumTv.ts`
const COMPOSABLE = `${ROOT}app/composables/usePlaybackSource.ts`
const UTILS = `${ROOT}app/utils/premiumTv.ts`
const SHIM = `${ROOT}app/utils/htmlvideo.ts`
const WATCH = `${ROOT}app/pages/live-tv/premium/watch.vue`
const GRID = `${ROOT}app/components/premium-tv/PremiumChannelGrid.vue`
const VOD_GRID = `${ROOT}app/components/premium-tv/PremiumVodGrid.vue`
const VOD_SIDEBAR = `${ROOT}app/components/premium-tv/PremiumVodSidebar.vue`
const MOVIE_PAGE = `${ROOT}app/pages/live-tv/premium/movie/[id].vue`
const CARD = `${ROOT}app/components/premium-tv/PremiumChannelCard.vue`
const EPG = `${ROOT}app/components/premium-tv/PremiumEpgPanel.vue`
const BROWSER = `${ROOT}app/components/premium-tv/PremiumBrowser.vue`
const HEADER = `${ROOT}app/components/live-tv/LiveBrowseHeader.vue`
const SIDEBAR = `${ROOT}app/components/premium-tv/PremiumSidebar.vue`
const ACCOUNT = `${ROOT}app/components/premium-tv/PremiumAccountCard.vue`
const CONNECT = `${ROOT}app/components/premium-tv/PremiumConnectForm.vue`

/** Everything the Premium TV front end is made of, for the sweeps below. */
const FRONTEND = [STORE, COMPOSABLE, UTILS, WATCH, GRID, VOD_GRID, VOD_SIDEBAR, MOVIE_PAGE, CARD, EPG, BROWSER, HEADER, SIDEBAR, ACCOUNT, CONNECT]

/** The markup files, where the TV rules apply. */
const TEMPLATES = [WATCH, GRID, VOD_GRID, VOD_SIDEBAR, MOVIE_PAGE, CARD, EPG, BROWSER, HEADER, SIDEBAR, ACCOUNT, CONNECT]

interface CheckResult {
  name: string
  passed: boolean
  detail?: string
}

const results: CheckResult[] = []

function check(name: string, fn: () => void) {
  try {
    fn()
    results.push({ name, passed: true })
  }
  catch (e) {
    results.push({ name, passed: false, detail: e instanceof Error ? e.message : String(e) })
  }
}

const src = new Map(FRONTEND.map(f => [f, readFileSync(f, 'utf8')]))
const read = (f: string) => src.get(f) ?? readFileSync(f, 'utf8')
const shimSrc = readFileSync(SHIM, 'utf8')
const storeSrc = read(STORE)
const watchSrc = read(WATCH)

// ── Reconnect: bounded, and backing off ──────────────────────────

check('reconnect is bounded by a small number of attempts', () => {
  assert.ok(Number.isInteger(MAX_RECONNECT_ATTEMPTS), 'MAX_RECONNECT_ATTEMPTS must be an integer')
  assert.ok(MAX_RECONNECT_ATTEMPTS >= 1, 'at least one attempt, or a blip kills the channel')
  assert.ok(MAX_RECONNECT_ATTEMPTS <= 8, `${MAX_RECONNECT_ATTEMPTS} attempts is an aggressive loop, not a retry`)
})

check('backoff grows and then caps', () => {
  const delays = Array.from({ length: MAX_RECONNECT_ATTEMPTS }, (_, i) => reconnectDelayMs(i + 1))
  assert.deepEqual(delays, [2000, 4000, 8000, 16_000].slice(0, MAX_RECONNECT_ATTEMPTS))
  for (let i = 1; i < delays.length; i++)
    assert.ok(delays[i]! >= delays[i - 1]!, 'a later attempt must not wait less than an earlier one')
  // Past the cap it stays put rather than growing into minutes.
  assert.equal(reconnectDelayMs(20), 20_000)
  /**
   * The first wait is two seconds, not one, and that is the point of the
   * schedule rather than a rounder number. A panel counts a *connection*
   * and does not stop counting the one that just died the instant our
   * player exits; an account with a single slot answers 401 to everything
   * until its own bookkeeping catches up. A schedule that spends every
   * attempt inside that window reports "dead channel" for something that
   * was only busy.
   */
  assert.ok(reconnectDelayMs(1) >= 2000, 'the first retry must clear a panel\'s slot-release window')
})

check('the whole retry budget is seconds, not minutes', () => {
  const total = Array.from({ length: MAX_RECONNECT_ATTEMPTS }, (_, i) => reconnectDelayMs(i + 1))
    .reduce((a, b) => a + b, 0)
  assert.ok(total <= 30_000, `a viewer waits ${total}ms before being told; that is too long`)
})

// ── The state machine lives in one place ─────────────────────────

check('clearing recently watched empties the grid, not just the sidebar count', () => {
  const fn = storeSrc.slice(storeSrc.indexOf('async function clearRecent'), storeSrc.indexOf('function setView'))
  assert.match(fn, /channels\.value = \[\]/, 'the grid reads channels, so that list has to go too')
  assert.ok(
    fn.indexOf('channels.value = []') < fn.indexOf('premiumApi.clearRecent'),
    'the page must empty before waiting on the API',
  )
})

check('the store owns all eight player states', () => {
  for (const state of ['idle', 'loading', 'playing', 'paused', 'buffering', 'reconnecting', 'error', 'ended']) {
    assert.ok(
      storeSrc.includes(`'${state}'`),
      `PremiumPlayerState must include '${state}'`,
    )
  }
  assert.ok(storeSrc.includes('export type PremiumPlayerState'), 'the union must be exported')
})

check('the connection machine is separate from the player machine', () => {
  assert.ok(/const connection = ref<'idle' \| 'loading' \| 'ready' \| 'error'>/.test(storeSrc))
  assert.ok(storeSrc.includes('const player = ref<PremiumPlayerState>'))
})

check('the watch page drives the store rather than keeping its own flags', () => {
  assert.ok(watchSrc.includes('premium.setPlayer('), 'transitions must go through setPlayer')
  assert.ok(watchSrc.includes('premium.nextReconnect()'), 'backoff must come from the store')
  assert.ok(!/\bconst (?:isPlaying|isBuffering|isReconnecting) = ref\(/.test(watchSrc), 'no parallel boolean state')
})

check('a spent retry budget ends in one clear error', () => {
  assert.ok(/delay === null/.test(watchSrc), 'the page must handle a null delay')
  assert.ok(/setPlayer\('error'/.test(watchSrc), 'and say so once, in the error state')
})

// ── One player, one HLS implementation ───────────────────────────

check('the duplicate premium player is gone', () => {
  assert.ok(
    !existsSync(`${ROOT}app/components/premium-tv/RivaltiWebLivePlayer.vue`),
    'RivaltiWebLivePlayer.vue is a second player; MpvPlayer + htmlvideo.ts is the one',
  )
})

check('no premium component implements playback itself', () => {
  for (const f of TEMPLATES) {
    const s = read(f)
    for (const own of ['hls.js', 'new Hls(', '<video']) {
      assert.ok(!s.includes(own), `${f.replace(ROOT, '')} implements its own playback (${own})`)
    }
  }
})

check('the watch page mounts the shared player with the right mode', () => {
  assert.ok(watchSrc.includes('<mpv-player'), 'the page must mount MpvPlayer')
  assert.ok(
    watchSrc.includes(':mode="playerMode"') || watchSrc.includes('mode="live"'),
    'live channels and vod titles must pick different player chrome',
  )
  assert.ok(watchSrc.includes(':user-agent="playback.source.value.userAgent"'), 'the upstream UA must reach the player')
  assert.ok(watchSrc.includes(':referer="playback.source.value.referer"'), 'and so must the referer')
  assert.ok(/@failed="[^"]*onPlaybackFailed/.test(watchSrc), 'a dead stream must reach the reconnect path')
  assert.ok(
    /@failed="reason =>/.test(watchSrc),
    'and it must carry mpv\'s reason, or a refusal cannot be told from silence',
  )
})

check('vod playback does not use the live reconnect loop', () => {
  const fn = watchSrc.slice(watchSrc.indexOf('function onPlaybackFailed'), watchSrc.indexOf('// ── Transport'))
  assert.ok(fn.includes('isVod'), 'playback failure must branch for movies and shows')
  assert.ok(
    watchSrc.includes('if (isVod.value)\n    return \'\''),
    'the live overlay must not duplicate vod error UI',
  )
})

check('paused live can jump back to the edge', () => {
  const overlay = readFileSync(`${ROOT}app/components/live-tv/LivePlayerOverlay.vue`, 'utf8')
  assert.ok(overlay.includes('behindLive'), 'the HUD must know when the user is behind live')
  assert.ok(overlay.includes('goLive'), 'and emit a jump')
  assert.ok(watchSrc.includes(':behind-live='), 'premium watch must pass that flag')
  assert.ok(watchSrc.includes('@go-live='), 'and wire the jump')
  assert.ok(shimSrc.includes('\'seek\''), 'the <video> shim must answer mpv seek')
})

/**
 * The bug that made every zap and every reconnect fail on a
 * single-connection account. A `:key` on the player unmounts the old
 * instance and mounts a new one, and the old one's `onBeforeUnmount`
 * fires `player_stop` *without awaiting it* while the new one is already
 * calling `player_start` — two mpv processes over one window, and a 401
 * for the slot the dying one has not released. Left unkeyed, both cases
 * go through the component's own `watch(src)`, which awaits the stop.
 */
check('the player is not remounted to change channel', () => {
  const tag = watchSrc.slice(watchSrc.indexOf('<mpv-player'), watchSrc.indexOf('/>', watchSrc.indexOf('<mpv-player')))
  assert.ok(!/:key=/.test(tag), 'a keyed player races its own teardown against the next start')
})

check('HLS is handled by the shared shim, natively where possible', () => {
  assert.ok(
    shimSrc.includes('canPlayType(\'application/vnd.apple.mpegurl\')'),
    'the shim must ask the webview before shipping a library',
  )
  const dynamic = shimSrc.indexOf('import(\'hls.js\')')
  assert.ok(dynamic > 0, 'hls.js must be imported dynamically')
  assert.ok(!/^import .*hls\.js/m.test(shimSrc), 'and never at module top level')
})

// ── Nothing about a provider reaches the browser ─────────────────

check('no premium file builds a provider URL or holds a credential', () => {
  for (const f of FRONTEND) {
    const s = read(f)
    for (const leak of ['player_api.php', 'panel_api.php', 'xmltv.php', 'get.php', '/live/', 'username=', 'password=']) {
      assert.ok(!s.includes(leak), `${f.replace(ROOT, '')} contains '${leak}'`)
    }
  }
})

check('the only playable URL is the loopback redirector', () => {
  assert.ok(read(UTILS).includes('127.0.0.1:3032'), 'the API base is loopback')
  assert.ok(read(COMPOSABLE).includes('premium-stream'), 'and the source it hands back is the redirector')
  assert.ok(!/https?:\/\/(?!127\.0\.0\.1|localhost)/.test(watchSrc), 'the watch page must name no remote host')
})

check('the API token is held in sessionStorage, not localStorage', () => {
  const s = read(UTILS)
  assert.ok(s.includes('sessionStorage'), 'the bearer must live in sessionStorage')
  assert.ok(!/localStorage\.[gs]etItem\(\s*['"`][^'"`]*(?:token|jwt)/i.test(s), 'and never in localStorage')
})

// ── Race safety ──────────────────────────────────────────────────

check('source resolution cancels and ignores late answers', () => {
  const s = read(COMPOSABLE)
  assert.ok(s.includes('AbortController'), 'must cancel the network work')
  assert.ok(s.includes('.abort()'), 'must actually abort')
  assert.ok(/requestId|reqId/.test(s), 'must ignore a response that was already decoding')
})

check('the channel list does the same', () => {
  assert.ok(storeSrc.includes('listController'), 'a paginated list needs its own controller')
  assert.ok(storeSrc.includes('listRequestId'), 'and its own request id')
})

// ── Scale ────────────────────────────────────────────────────────

check('the grid is virtualized', () => {
  for (const f of [GRID, VOD_GRID]) {
    const s = read(f)
    assert.ok(s.includes('@tanstack/vue-virtual'), `${f.replace(ROOT, '')} must use a virtualizer`)
    assert.ok(s.includes('measureElement'), `${f.replace(ROOT, '')} rows are measured`)
    assert.ok(s.includes('overscan'), `${f.replace(ROOT, '')} keeps a small overscan for the remote`)
  }
})

check('vod browse opens a movie detail page before play', () => {
  const browser = read(BROWSER)
  assert.ok(/premium\/movie\//.test(browser) || browser.includes('openMovie'), 'movies must open a detail page')
  assert.ok(read(MOVIE_PAGE).includes('@click="play"'), 'the detail page must offer Play')
})

check('vod category filter uses search-field', () => {
  const s = read(VOD_SIDEBAR)
  assert.ok(s.includes('search-field'), 'the VOD rail must use SearchField')
  assert.ok(!/<input[^>]*type="search"/.test(s), 'no raw search input in the VOD rail')
})

check('nothing renders the whole channel list', () => {
  for (const f of [BROWSER, SIDEBAR]) {
    const s = read(f)
    assert.ok(
      !/v-for="[^"]*\bin (?:premium\.)?channels\b/.test(s),
      `${f.replace(ROOT, '')} iterates the channel list directly`,
    )
  }
})

check('logos are lazy and the guide is fetched in batches', () => {
  assert.ok(read(CARD).includes('loading="lazy"'), 'a thousand logos must not all load at once')
  const s = read(GRID)
  assert.ok(/EPG_BATCH\s*=\s*\d+/.test(s), 'now/next must be asked for in batches')
  assert.ok(/EPG_DEBOUNCE_MS\s*=\s*\d+/.test(s), 'and not once per scroll frame')
})

// ── The guide degrades to nothing ────────────────────────────────

check('an empty guide renders no container at all', () => {
  const s = read(EPG)
  assert.ok(/const empty = computed/.test(s), 'the panel must decide when it has nothing')
  assert.ok(s.includes('v-if="!empty"'), 'and render nothing then, not an empty box')
})

check('the watch page hides the panel rather than showing an empty one', () => {
  assert.ok(
    /guide\.length > 0 \|\| guideLoading/.test(watchSrc),
    'the panel is mounted only with programmes or a fetch in flight',
  )
})

// ── The remote ───────────────────────────────────────────────────

check('every hover: has a focus-visible: twin on the same element', () => {
  for (const f of TEMPLATES) {
    const s = read(f)
    const template = s.slice(s.indexOf('<template>'))
    // Split on tag starts: the twin has to be on the element that hovers,
    // and a `:class` binding beside a static `class` is the same element.
    for (const chunk of template.split(/(?=<[a-z])/i)) {
      if (!chunk.includes('hover:'))
        continue
      assert.ok(
        chunk.includes('focus-visible:') || chunk.includes('focus:'),
        `${f.replace(ROOT, '')}: an element with hover: styling has no focus twin`,
      )
    }
  }
})

check('the interactive parts are real buttons', () => {
  for (const f of [CARD, SIDEBAR, BROWSER, HEADER]) {
    const s = read(f)
    assert.ok(s.includes('type="button"'), `${f.replace(ROOT, '')} must use real buttons`)
  }
})

check('the decorative overlay button stays out of the tab order', () => {
  assert.ok(read(CARD).includes('tabindex="-1"'), 'the card\'s favourite star is inside a button and must not focus')
})

check('the channel card focus ring is inset on the artwork', () => {
  const s = read(CARD).replace(/<!--[\s\S]*?-->/g, '')
  assert.ok(s.includes('ring-inset'), 'the hover ring must sit inside the logo box')
  assert.ok(!/hover:ring/.test(s), 'an outside hover:ring boxed the title under the art')
})

check('the sidebar selected state uses the theme primary, not a second colour system', () => {
  const s = read(SIDEBAR)
  assert.ok(!s.includes('amber-500'), 'Favorites must not be amber')
  assert.ok(!s.includes('cyan-500'), 'Recently watched must not be cyan')
  assert.ok(!s.includes('text-gray'), 'the rail uses on-surface tokens, not Tailwind gray')
  assert.ok(s.includes('bg-primary'), 'the selected item is primary')
})

check('the category filter is a field a d-pad can walk past', () => {
  assert.ok(read(SIDEBAR).includes('search-field'), 'the rail must use SearchField, not a raw input')
})

check('the browse search is a field a d-pad can walk past', () => {
  assert.ok(read(HEADER).includes('search-field'), 'the header must use SearchField, not a raw input')
  assert.ok(read(BROWSER).includes('live-tv-live-browse-header'), 'Premium browse uses the shared header')
  assert.ok(!read(BROWSER).includes('v-text-field'), 'Premium browse must not mount a raw text field')
})

check('provider folder names drop the Live stamp and the two-letter prefix', () => {
  assert.deepEqual(parseCategoryName('AF - Canal+ Africa - Live'), { code: 'AF', label: 'Canal+ Africa' })
  assert.deepEqual(parseCategoryName('AR - Arabic TV - Live'), { code: 'AR', label: 'Arabic TV' })
  assert.deepEqual(parseCategoryName('ARG - Argentina'), { code: 'ARG', label: 'Argentina' })
  assert.deepEqual(parseCategoryName('News'), { code: null, label: 'News' })
  assert.equal(isBundleCategory('ALL SPORTS 4K'), true)
  assert.equal(isBundleCategory('ALL MOVIES'), true)
  assert.equal(isBundleCategory('ALGERIA'), false)
  assert.equal(categoryLabel('ALL SPORTS 4K'), 'All Sports 4K')
})

check('the guide progress bar is announced', () => {
  const s = read(EPG)
  assert.ok(s.includes('role="progressbar"'), 'progress needs a role')
  assert.ok(s.includes('aria-valuenow'), 'and a value a screen reader can read')
})

// ── i18n ─────────────────────────────────────────────────────────

check('no premium string bypasses $t()', () => {
  for (const f of TEMPLATES) {
    const s = read(f)
    // A user-facing string assigned to `error.value` is the one that got
    // away twice; anything else in a template is caught by check:i18n.
    for (const m of s.matchAll(/error\.value = (['"`])(?!\s*\1)/g)) {
      const line = s.slice(0, m.index).split('\n').length
      assert.fail(`${f.replace(ROOT, '')}:${line} assigns a bare string to error.value`)
    }
  }
})

// ── Report ───────────────────────────────────────────────────────

const passed = results.filter(r => r.passed).length
const failed = results.length - passed
for (const r of results) {
  const tag = r.passed ? '✓' : '✗'
  console.log(`${tag} ${r.name}${r.passed ? '' : ` — ${r.detail}`}`)
}
console.log(`\n${passed} passed, ${failed} failed`)
if (failed === 0)
  console.log('premium player: ok')
process.exit(failed > 0 ? 1 : 0)
