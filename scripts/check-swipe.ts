// Self-check for the drawer's edge swipe and the player's volume / brightness
// swipes: `bun scripts/check-swipe.ts`.
//
// The numbers here are measured, not chosen: on a Pixel 8 Pro a touch starting
// at CSS x≤27 is swallowed by Android's back gesture and never reaches the
// webview at all, which is why the band cannot simply start at 0 — and why
// Vuetify's own 0-25px zone is dead on that platform.
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { clampLevel, edgeAdjust, edgeDelta, isHorizontalSwipe, isVerticalSwipe, seekSeconds } from '../app/utils/playerSwipe'
import { inSwipeZone, opensDrawer, SWIPE_FROM } from '../app/utils/swipe'

// --- The band ---------------------------------------------------------------

assert.ok(SWIPE_FROM > 27, 'the band has to start clear of Android\'s back gesture')

assert.ok(!inSwipeZone(0), 'the very edge belongs to the OS')
assert.ok(!inSwipeZone(27), 'so does the rest of the system gesture band')
assert.ok(inSwipeZone(40), 'just inside is ours')
assert.ok(inSwipeZone(95), 'and so is the far side of the band')
assert.ok(!inSwipeZone(200), 'a drag from the middle of the page is not a drawer swipe')

// --- The drag ---------------------------------------------------------------

assert.ok(opensDrawer(90, 4), 'a flat drag to the right opens it')
assert.ok(!opensDrawer(20, 2), 'a twitch does not')
assert.ok(!opensDrawer(-90, 4), 'nor does a drag the other way')

// Scrolling the page starts at some x too, and often drifts sideways doing it.
assert.ok(!opensDrawer(90, 140), 'mostly-vertical is a scroll, not an open')
assert.ok(!opensDrawer(70, 70), 'a 45° drag is ambiguous, so it is a scroll')

console.info('drawer swipe: ok')

// --- Player picture: left = volume, right = brightness --------------------

assert.equal(edgeAdjust(10, 100), 'volume', 'left 40% is volume')
assert.equal(edgeAdjust(39, 100), 'volume')
assert.equal(edgeAdjust(50, 100), null, 'the middle third stays a tap')
assert.equal(edgeAdjust(61, 100), 'brightness', 'right 40% is brightness')
assert.equal(edgeAdjust(99, 100), 'brightness')
assert.equal(edgeAdjust(0, 0), null)

assert.equal(edgeDelta(-100, 100), 100, 'a full-height swipe up is +100')
assert.equal(edgeDelta(100, 100), -100, 'and down is −100')
assert.equal(edgeDelta(0, 100), 0)
assert.equal(edgeDelta(-50, 0), 0)

assert.equal(clampLevel(-4), 0)
assert.equal(clampLevel(140), 100)
assert.equal(clampLevel(33.4), 33)

assert.ok(isVerticalSwipe(0, 20), 'a tall drag is a swipe')
assert.ok(!isVerticalSwipe(0, 10), 'a tap is not')
assert.ok(!isVerticalSwipe(40, 20), 'nor is a mostly-horizontal drag')

assert.ok(isHorizontalSwipe(40, 0), 'a sideways drag is a seek')
assert.ok(!isHorizontalSwipe(10, 0), 'a twitch is not')
assert.ok(!isHorizontalSwipe(20, 40), 'nor is a mostly-vertical drag')

assert.equal(seekSeconds(100, 100, 1000), 90, 'one screen is at most 90s')
assert.equal(seekSeconds(100, 100, 100), 20, 'a short title still moves 20s per screen')
assert.equal(seekSeconds(50, 100, 1000), 45)
assert.equal(seekSeconds(10, 0, 100), 0)

const edge = readFileSync(new URL('../app/composables/usePlayerEdgeSwipe.ts', import.meta.url), 'utf8')
assert.ok(!edge.includes('pointerType === \'mouse\''), 'Android WebView reports a finger as mouse')
assert.ok(edge.includes('addEventListener(\'pointermove\''), 'moves are tracked on window, not only the picture')
assert.ok(edge.includes('addEventListener(\'touchmove\''), 'and on touchmove, which is what already works for the drawer')

const mpv = readFileSync(new URL('../app/components/MpvPlayer.vue', import.meta.url), 'utf8')
assert.ok(mpv.includes('usePlayerEdgeSwipe'), 'the film player wires the same swipe')
assert.ok(mpv.includes('onPictureTouchStart'), 'Android starts the drag from touchstart, like the drawer')
assert.ok(mpv.includes('seek:'), 'films scrub on a horizontal drag')

const live = readFileSync(new URL('../app/components/live-tv/LivePlayerOverlay.vue', import.meta.url), 'utf8')
assert.ok(live.includes('usePlayerEdgeSwipe'), 'live TV uses it too')
assert.ok(live.includes('onCentrePointerDown'))
assert.ok(live.includes('onCentreTouchStart'), 'live TV starts from touchstart too')

const platform = readFileSync(new URL('../app/utils/platform.ts', import.meta.url), 'utf8')
assert.ok(platform.includes('setMediaVolume'), 'volume swipe talks to STREAM_MUSIC')
assert.ok(platform.includes('setScreenBrightness'), 'brightness swipe talks to the window')

const activity = readFileSync(
  new URL('../src-tauri/gen/android/app/src/main/java/io/github/rivulet/rivulet/MainActivity.kt', import.meta.url),
  'utf8',
)
assert.ok(activity.includes('fun mediaVolume()'), 'MainActivity answers mediaVolume()')
assert.ok(activity.includes('fun setMediaVolume'), 'and setMediaVolume()')
assert.ok(activity.includes('fun brightness()'), 'and brightness()')
assert.ok(activity.includes('fun setBrightness'), 'and setBrightness()')
assert.ok(activity.includes('STREAM_MUSIC'), 'volume is the music stream the rocker uses')

const drawer = readFileSync(new URL('../app/plugins/drawerswipe.client.ts', import.meta.url), 'utf8')
assert.ok(drawer.includes('rivulet-video'), 'the drawer must not steal volume from the player')

console.info('player swipe: ok')
