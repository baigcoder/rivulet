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

console.info('free tv health: ok')
