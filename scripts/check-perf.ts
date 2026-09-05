// Self-check for what keeps the app drawable on a television:
// `bun scripts/check-perf.ts`.
//
// None of this is logic with a return value to assert on — it is CSS, and the
// only thing that catches a regression is measuring frames on a real TV, which
// no test can do. What it *can* do is hold the shape of the fix in place, since
// every rule here was reverted one at a time on the test set and the frame rate
// measured, and every one of them is a line another change would remove without
// noticing: an effect looks free on the laptop it was written on.
//
// Measured on a Philips TPM191E (1.5GHz quad A53, 1080p), release build, home
// page loaded to 278 cards — none of this / always-on half / switch as well:
//
//   scrolling the page      16.1 → 36.0 → 39.4 fps
//   ten d-pad moves          3.1 → 13.1 → 22.8 fps
//
// The always-on half is the bigger win and costs nothing to look at. The switch
// is worth having anyway: it nearly doubles what a remote feels like, because
// moving focus is what fires the transitions and scrolling largely doesn't.
import assert from 'node:assert'
import { readdirSync, readFileSync } from 'node:fs'
import process from 'node:process'
import { youtubeCommand, youtubeError, youtubePlaying } from '../app/utils/youtube'

const read = (path: string) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8')

const card = read('app/components/MediaCard.vue')
const layers = read('app/assets/css/layers.css')
const background = read('app/components/AppBackground.vue')
const settings = read('app/stores/settings.ts')
const appearance = read('app/pages/settings/appearance/display.vue')
const app = read('app/app.vue')
const row = read('app/components/ScrollRow.vue')

// --- Always on, no setting: these cost nothing to look at ---

// Worth 2x on its own — the browse pages mount hundreds of cards and without it
// a TV paints every one of them, on screen or not.
assert.match(
  card,
  /\[content-visibility:auto\]/,
  'MediaCard must skip rendering off-screen cards (content-visibility)',
)
assert.match(
  card,
  /containIntrinsicSize/,
  'content-visibility needs a size to reserve, or the scrollbar jumps as cards are drawn',
)
assert.match(
  card,
  /finger\.value/,
  'a phone tap must not count as hover: that turns content-visibility off and mounts the overlay',
)

const layout = read('app/components/MediaLayout.vue')
assert.match(
  layout,
  /minmax\(min\(/,
  'the browse grid must shrink below cardWidth so a phone gets two posters, not one',
)

// The hover/focus ring is inset on the poster. A box-shadow or outline on the
// wrapping <a> boxed the title under the art — the caption is not the poster.
assert.match(
  card,
  /ring-inset/,
  'the hover ring must sit inside the poster, not around the whole card',
)
assert.match(
  card,
  /v-if="detail"/,
  'the title under the poster is outside the ring',
)
assert.doesNotMatch(
  card.replace(/<!--[\s\S]*?-->|\/\/[^\n]*/g, ''),
  /hover\.value \? '1\.05'/,
  'no whole-card hover scale: a TV paints that transform on every focus move',
)

// One frame-buffer readback per card, twenty on screen at once. Comments are
// stripped first — the one above the badge says the word to explain its absence.
assert.doesNotMatch(
  card.replace(/<!--[\s\S]*?-->|\/\/[^\n]*/g, ''),
  /backdrop-blur/,
  'no backdrop-filter on a card: it is per-card GPU work for an effect too small to see',
)

// The blurred art never moves, but it sits behind a scrolling page — without its
// own compositor layer the blur is redone every frame.
assert.match(background, /rivulet-backdrop/, 'the backdrop art needs the class the CSS promotes')
assert.match(
  layers,
  /\.rivulet-backdrop\s*\{[^}]*will-change:\s*transform/,
  'the backdrop art must be promoted to its own layer so its blur is cached',
)

// --- Behind the switch ---

for (const [what, re] of [
  ['transitions', /html\.reduce-effects \*[\s\S]{0,200}?transition:\s*none\s*!important/],
  ['the frosted chrome', /html\.reduce-effects[\s\S]{0,400}?backdrop-filter:\s*none/],
  ['the backdrop blur', /html\.reduce-effects \.rivulet-backdrop\s*\{[^}]*filter:/],
] as const)
  assert.match(layers, re, `reduce-effects must drop ${what}`)

// A frozen spinner reads as a hung app, and the skeletons are the only other
// animation here — so the switch deliberately leaves `animation` alone. Costs
// nothing either: measured, the frame rate came back from transitions, not these.
assert.doesNotMatch(
  layers,
  /html\.reduce-effects[\s\S]{0,200}?animation:\s*none/,
  'reduce-effects must not stop animations — the loading spinners are animations',
)

// The blur goes, the brightness and saturation stay: they are what stops white
// poster art washing the text out, which is a legibility bug, not a slow frame.
assert.match(
  layers,
  /html\.reduce-effects \.rivulet-backdrop\s*\{[^}]*brightness\([^)]*\)[^}]*saturate\(/,
  'dropping the backdrop blur must keep its brightness/saturation',
)

// --- The rows ---

// A card crossing under a stationary cursor mounts the hover overlay, moves the
// backdrop and comes out of content-visibility — a whole row of that per flick
// of the wheel, which is what made paging a row flicker and shift+wheel stutter.
// On the track, not the scroller: a scroller that ignores the pointer never sees
// the wheel either, and the page would scroll instead of the row.
assert.match(
  row,
  /:class="\{ 'pointer-events-none': gliding \}"/,
  'a moving row must take its cards out of the pointer\'s way',
)
assert.doesNotMatch(
  row.replace(/<!--[\s\S]*?-->/g, ''),
  /snap-x|snap-start/,
  'no scroll-snap on a row: it re-animates after every wheel notch and every appended page',
)

// `useScroll` measures on mount and on scroll only, and a row is empty when it
// mounts — both arrows came up disabled and stayed there until it was dragged.
assert.match(row, /useResizeObserver\(\[scroller, track\], measure\)/, 'the arrows must re-measure as cards arrive')

// --- The episode list ---

// A thousand-episode season is a real thing (anime), and a row apiece — a
// Vuetify button, a tooltip and a watched dialog each — froze the page on
// open and left three thousand focusables for the d-pad to measure a press.
// The window is bounded by the viewport instead, as the live-tv grids do it.
const liveCard = read('app/components/live-tv/LiveChannelCard.vue')
assert.match(
  liveCard,
  /\[content-visibility:auto\]/,
  'live channel cards must skip painting off-screen work the same way posters do',
)
assert.doesNotMatch(
  liveCard.replace(/<!--[\s\S]*?-->|\/\/[^\n]*/g, ''),
  /hover:-translate-y/,
  'no per-card lift on the live grid: a TV paints that transform on every focus move',
)
assert.match(
  liveCard,
  /ring-inset/,
  'the hover ring must sit inside the channel artwork, not around the whole card',
)
assert.doesNotMatch(
  liveCard.replace(/<!--[\s\S]*?-->|\/\/[^\n]*/g, ''),
  /hover:ring/,
  'no whole-card hover:ring on live tiles: that boxed the title the same way posters used to',
)
assert.doesNotMatch(
  liveCard.replace(/<!--[\s\S]*?-->|\/\/[^\n]*/g, ''),
  /hover:scale/,
  'no per-card scale on the live grid',
)

const premiumCard = read('app/components/premium-tv/PremiumChannelCard.vue')
assert.match(
  premiumCard,
  /ring-inset/,
  'Premium tiles share the inset-ring contract with Free TV',
)
assert.doesNotMatch(
  premiumCard.replace(/<!--[\s\S]*?-->|\/\/[^\n]*/g, ''),
  /hover:ring/,
  'no whole-card hover:ring on Premium tiles',
)
assert.doesNotMatch(
  premiumCard.replace(/<!--[\s\S]*?-->|\/\/[^\n]*/g, ''),
  /\[content-visibility:auto\]/,
  'Premium cards must not use content-visibility: the grid measures row height',
)

const liveHub = read('app/pages/live-tv/index.vue')
assert.doesNotMatch(
  liveHub.replace(/<!--[\s\S]*?-->|\/\/[^\n]*/g, ''),
  /blur-2xl/,
  'the Live TV hub must not paint decorative blur orbs',
)
assert.match(liveHub, /rounded-2xl/, 'hub tiles follow the home teaser, not marketing cards')

assert.match(
  read('app/pages/[type]/[id].vue'),
  /<media-detail-view/,
  'the library title route must render the shared title page',
)
const detail = read('app/components/MediaDetailView.vue')
assert.match(
  detail,
  /setTimeout\(go, 4000\)/,
  'the YouTube hero iframe must wait after first paint — requestIdleCallback fires too soon',
)
assert.match(
  detail,
  /heroIdle/,
  'the trailer src must stay empty until the page has painted',
)
assert.match(
  detail,
  /<v-lazy/,
  'cast, seasons and recommendations on the title page must lazy-mount',
)
assert.doesNotMatch(
  detail.replace(/<!--[\s\S]*?-->|\/\/[^\n]*/g, ''),
  /backdrop-blur/,
  'no backdrop-filter on the title hero: it is a GPU readback on the first frame',
)
assert.match(
  detail,
  /ui\.opening/,
  'the title page must paint the card snapshot while TMDB is still in flight',
)
assert.match(
  detail,
  /backdropUrl\([^)]*'w780'/,
  'the hero backdrop is w780: w1280 is a second decode on the first frame',
)
assert.match(
  detail,
  /heroPoster/,
  'the hero must paint the cached card poster before the backdrop decodes',
)
assert.match(
  detail,
  /requestAnimationFrame/,
  'select() must wait for the first paint: it decodes the window backdrop and may retheme',
)
assert.match(
  read('app/components/MediaCard.vue'),
  /ui\.open\(/,
  'a card press snapshots the title only — select() on pointerdown hitchs the click',
)
assert.match(
  detail,
  /media\.value\?\.seasons \?\? \[\]/,
  'Play on a show must not wait for the TMDB detail to name S1 E1',
)
const tmdb = read('app/utils/tmdb.ts')
assert.doesNotMatch(
  tmdb,
  /new Image\(\)/,
  'prefetch must not decode a backdrop on the click that opens the title',
)
assert.match(tmdb, /DETAIL_CORE/, 'the first title request must stay small')
assert.doesNotMatch(tmdb, /DETAIL_APPEND/, 'the fat credits\+images append must not ride with first paint')
assert.match(tmdb, /prefetchMediaDetail/, 'a card press must start the title request before the page mounts')
assert.match(tmdb, /getCachedData/, 'a prefetched title must be on the page on the first frame')
assert.doesNotMatch(
  read('app/components/MediaCard.vue'),
  /preloadRouteComponents/,
  'do not preload the title route from a card: Vite locks the WebView',
)
assert.match(tmdb, /immediate: false/, 'credits must not start until the title request has landed')
assert.match(tmdb, /extra\.execute/, 'credits start only after the title record is in hand')
assert.doesNotMatch(
  tmdb.replace(/\/\*[\s\S]*?\*\//g, ''),
  /\$\{[^}]+\}\/images/,
  'do not fetch /images on open: it is every poster and backdrop',
)
assert.match(
  detail,
  /media-images/,
  'the title page shows stills after first paint, not in the hero request',
)
assert.match(
  read('app/utils/titleImages.ts'),
  /include_image_language/,
  'the stills request must not ask for every language poster',
)
assert.match(
  detail,
  /unMute/,
  'the hero volume button must talk to the player, not rebuild the iframe src',
)
assert.doesNotMatch(
  detail.replace(/<!--[\s\S]*?-->|\/\/[^\n]*/g, ''),
  /youtubeEmbedSrc\([^)]*heroMuted/,
  'mute must not sit in the hero src: changing it reloads YouTube from the start',
)
assert.match(detail, /hd720/, 'the hero must lock YouTube at 720p')
assert.match(detail, /heroPlaying/, 'the hero iframe stays hidden until YouTube is playing')
assert.match(detail, /pauseVideo/, 'the hero must pause when scrolled off the page')
assert.match(detail, /useIntersectionObserver/, 'hero pause is driven by the hero box leaving the viewport')
assert.match(detail, /nextTrailer/, 'a geo-blocked YouTube key must fall through to the next TMDB trailer')
assert.match(detail, /youtubeError/, 'YouTube onError must skip the blocked embed, not paint the country card')

const youtube = read('app/utils/youtube.ts')
assert.match(youtube, /vq:\s*['"]hd720['"]/, 'browser embeds request 720p')
assert.match(read('src-tauri/src/iptv/proxy.rs'), /vq=hd720/, 'the Tauri YouTube relay requests 720p')
assert.equal(youtubeCommand('setPlaybackQuality', ['hd720']), '{"event":"command","func":"setPlaybackQuality","args":["hd720"]}')
assert.equal(youtubePlaying('{"info":{"playerState":1}}'), true)
assert.equal(youtubePlaying('{"info":{"playerState":3}}'), false)
assert.equal(youtubeError('{"event":"onError","info":150}'), true)
assert.equal(youtubeError('{"info":{"playerState":1}}'), false)

const seasonPage = read('app/pages/tv/[id]/season/[season]/index.vue')
assert.match(
  seasonPage,
  /useVirtualizer/,
  'the episode list must be virtualized: mounting every row is what froze a 1000-episode season',
)
assert.match(
  seasonPage,
  /measureElement/,
  'rows are measured as they mount, or estimates drift and the scrollbar jumps',
)
assert.match(
  seasonPage,
  /scrollMargin/,
  'the header scrolls with the list, so the virtualizer needs to know where the list starts',
)

// --- Wiring: a setting nothing reads is a setting that does nothing ---

// The key is built by `key()` from app/brand now rather than spelled out, so the
// prefix moves with the product name and this asserts the shape, not the string.
assert.match(settings, /reduceEffects = useLocalStorage\(key\('reduceEffects'\)/, 'the setting is stored')
assert.match(settings, /isTv\(\) === true \|\| isAndroid\(\)/, 'a television or a phone gets it on by default')
assert.match(settings, /return \{[^}]*reduceEffects/, 'the store must expose it')
assert.match(
  app,
  /classList\.toggle\('reduce-effects', settings\.reduceEffects\)/,
  'the setting must put the class on <html>, which is what the CSS keys off',
)
assert.match(
  app,
  /classList\.toggle\('android', isAndroid\(\)\)/,
  'a phone WebView must be marked so CSS can drop backdrop-filter without the switch',
)
assert.match(
  layers,
  /html\.android[\s\S]{0,280}?backdrop-filter:\s*none/,
  'Android must not run backdrop-filter: WebView readback is the lag',
)
assert.match(appearance, /v-model="settings\.reduceEffects"/, 'Appearance needs the switch')

const activity = read(
  'src-tauri/gen/android/app/src/main/java/io/github/rivulet/rivulet/MainActivity.kt',
)
assert.match(activity, /offscreenPreRaster/, 'the WebView must raster tiles before they scroll on')
assert.match(activity, /LAYER_TYPE_HARDWARE/, 'and stay on the GPU, not a software copy')
assert.match(activity, /OVER_SCROLL_NEVER/, 'overscroll glow is a full-surface stretch on a phone')

/* --- Motion ----------------------------------------------------------------

   Every entrance in the app is a keyframe animation rather than a transition,
   and that is a performance decision before it is a design one. `reduce-effects`
   above kills `transition` outright and is on by default on a television, so a
   transition-based reveal is dead code on the one device that needed the care.

   What makes keyframes affordable there is *which* properties they touch. The
   profile in this file's header put paint and raster at essentially the whole
   frame; `transform` and `opacity` are handled by the compositor and repaint
   nothing, so they are close to free even on a 1.5GHz A53. Anything else in a
   keyframe is the bottleneck itself.

   These asserts exist because that distinction is invisible in review. A
   `filter` or a `box-shadow` added to a keyframe looks like one more line of
   polish on the laptop it was written on, and costs a third of the frame rate
   on the set. */

const COMPOSITED = new Set(['transform', 'opacity', 'animation-timing-function'])

/** `@keyframes name { … }`, one level of nesting — which is all keyframes have. */
const KEYFRAMES = /@keyframes\s+([\w-]+)\s*\{((?:[^{}]|\{[^{}]*\})*)\}/g

/** Every stylesheet and component style block under app/. */
function styles(dir = 'app'): { path: string, text: string }[] {
  const out: { path: string, text: string }[] = []
  for (const entry of readdirSync(new URL(`../${dir}/`, import.meta.url), { withFileTypes: true })) {
    const at = `${dir}/${entry.name}`
    if (entry.isDirectory())
      out.push(...styles(at))
    else if (/\.(?:css|vue)$/.test(entry.name))
      out.push({ path: at, text: read(at) })
  }
  return out
}

let animations = 0
for (const { path, text } of styles()) {
  for (const [, name, body] of text.matchAll(KEYFRAMES)) {
    animations++
    for (const [, property] of body!.matchAll(/(?:^|[{;\s])([a-z][\w-]*)\s*:/g)) {
      assert.ok(
        COMPOSITED.has(property!),
        `${path}: @keyframes ${name} animates \`${property}\`, which repaints. `
        + 'Keyframes here may only touch transform and opacity — see this block\'s comment.',
      )
    }
  }
}
assert.ok(animations >= 4, `expected the motion keyframes to exist, found ${animations}`)

// Promoting an element caches its paint, which is why the backdrop has it — and
// why nothing else may. A few hundred promoted cards is a few hundred textures
// on a device whose whole budget is paint.
assert.equal(
  [...layers.matchAll(/will-change/g)].length,
  1,
  'will-change belongs to the backdrop alone — promoting in bulk trades paint for VRAM',
)
for (const { path, text } of styles()) {
  if (path !== 'app/assets/css/layers.css')
    assert.doesNotMatch(text, /will-change/, `${path}: promotion is decided in layers.css, not per component`)
}

// The tiers. `full` needs no rule of its own — it is the bare `:root` defaults —
// so only the two that override anything are asserted.
for (const token of ['reveal', 'trace', 'clip', 'fade', 'stagger'])
  assert.match(layers, new RegExp(`--motion-${token}:`), `the motion vocabulary needs --motion-${token}`)

for (const tier of ['reduced', 'none']) {
  const block = new RegExp(`html\\.motion-${tier}\\s*\\{([^}]*)\\}`).exec(layers)
  assert.ok(block, `layers.css must define the motion-${tier} tier`)
  // Redefining the properties is the whole mechanism: a tier that reached for
  // `animation: none !important` instead would also stop the loading spinners,
  // which is the same mistake reduce-effects deliberately avoids above.
  for (const [, property] of block![1]!.matchAll(/([a-z-][\w-]*)\s*:/g)) {
    assert.ok(
      property!.startsWith('--motion-'),
      `motion-${tier} sets \`${property}\` — a tier may only redefine the --motion-* properties`,
    )
  }
}

assert.match(layers, /--motion-stagger:\s*0ms/, 'a reduced tier must not still stagger')

// Wiring, same reasoning as reduce-effects above. Matched loosely on purpose:
// `eslint --fix` owns the line breaks in that nested ternary, and an assert that
// pins its formatting fails the next time the formatter changes its mind.
assert.match(settings, /key\('motion'\), 'system'\)/, 'the motion mode is stored, defaulting to Automatic')
assert.match(
  settings,
  /prefers-reduced-motion: reduce/,
  'Automatic must honour the operating system — this is the only place the app reads it',
)
assert.match(settings, /isTv\(\) === true/, 'and a television must be asked too')
assert.match(settings, /'reduced'/, 'both of those resolve to the reduced tier')
assert.match(settings, /return \{[^}]*effectiveMotion/, 'the store must expose the resolved tier')
assert.match(app, /motion-\$\{tier\}/, 'the resolved tier must put its class on <html>')
assert.match(app, /settings\.effectiveMotion === tier/, 'exactly one tier class at a time')
assert.match(appearance, /v-model="settings\.motion"/, 'Appearance needs the motion control')

// eslint-disable-next-line no-console
console.log('check-perf: ok')
process.exit(0)
