// Self-check for the update notice: `bun scripts/check-updates.ts`.
//
// Two things worth holding still. The version compare decides whether people
// are told about a release at all — get it backwards and the app either nags
// for ever about a version it already runs, or never mentions one. And the
// GitHub release shape is somebody else's to change: a renamed field that
// quietly parsed to an empty version would turn the badge off with nothing
// anywhere to notice.
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { compareVersions, isNewer, parseUpdate, pickApk, RELEASES_URL, transportMessage } from '../app/utils/updates'
import './i18n-stub'

// --- Ordering ----------------------------------------------------------------

const older = (a: string, b: string) => assert.ok(compareVersions(a, b) < 0, `${a} < ${b}`)

older('0.1.1', '0.2.0')
older('0.9.0', '0.10.0') // the one a plain string compare gets wrong
older('1.0.0', '1.0.1')
older('0.1.0', '1.0.0')
assert.equal(compareVersions('0.2.0', '0.2.0'), 0)

// A tag carries the `v`; `getVersion()` never does. Both have to compare equal.
assert.equal(compareVersions('v0.2.0', '0.2.0'), 0)
// Build metadata is explicitly not part of the ordering.
assert.equal(compareVersions('0.2.0+abc', '0.2.0'), 0)
// Short forms count the missing parts as zero rather than as anything else.
assert.equal(compareVersions('1.2', '1.2.0'), 0)

// --- Prereleases -------------------------------------------------------------
// `releases/latest` never returns one, so this only matters on the side the
// *user* is running: someone on an rc has to be offered the real release.

older('0.2.0-rc.1', '0.2.0')
older('0.2.0-alpha', '0.2.0-beta')
older('0.2.0-rc.9', '0.2.0-rc.10') // numeric identifiers compare as numbers
older('0.2.0-rc', '0.2.0-rc.1') // fewer identifiers is the lower one
older('0.2.0-1', '0.2.0-alpha') // numeric sorts below alphanumeric
older('0.1.9', '0.2.0-rc.1')

// --- What actually gets offered ----------------------------------------------

assert.ok(isNewer('0.1.1', '0.2.0'))
assert.ok(!isNewer('0.2.0', '0.2.0'), 'the version you are running is not an update')
assert.ok(!isNewer('0.3.0', '0.2.0'), 'never offer a downgrade')
// No version means no Tauri — a browser dev session. Nothing to offer there.
assert.ok(!isNewer('', '0.2.0'))
assert.ok(!isNewer('0.1.1', ''))

// --- The GitHub payload ------------------------------------------------------

const release = {
  tag_name: 'v0.2.0',
  body: '  ## What changed\n- things\n  ',
  html_url: 'https://github.com/baigcoder/rivulet/releases/tag/v0.2.0',
  draft: false,
  prerelease: false,
  assets: [
    { name: 'Rivulet_0.2.0_amd64.AppImage', browser_download_url: 'https://example.invalid/appimage' },
    { name: 'Rivulet_0.2.0.apk', browser_download_url: 'https://example.invalid/apk' },
  ],
}

const parsed = parseUpdate(release)
assert.equal(parsed?.version, '0.2.0', 'the tag\'s v is not part of the version')
assert.equal(parsed?.notes, '## What changed\n- things')
assert.equal(parsed?.url, release.html_url)
// Android needs the one file it can install, not the six the release carries.
assert.equal(parsed?.apk, 'https://example.invalid/apk')

// --- One APK per architecture ------------------------------------------------
// A universal APK is every ABI's native libraries in one file: measured at
// 311 MB on a phone, of which 293 MB was three copies of `librivulet_lib.so`
// (59.8 MB) and `libvlc.so` (39.8 MB). A device loads exactly one of the three
// sets, and the phone measured never opens the x86_64 copy — the largest of
// them. So a release now carries a per-ABI build beside the universal one.
//
// Picking the wrong one is not a slower download, it is an app that will not
// install, so this is the part that has to be right.
const split = [
  { name: 'Rivulet_0.2.0.apk', browser_download_url: 'https://example.invalid/universal' },
  { name: 'Rivulet_0.2.0-arm64-v8a.apk', browser_download_url: 'https://example.invalid/arm64' },
  { name: 'Rivulet_0.2.0-armeabi-v7a.apk', browser_download_url: 'https://example.invalid/arm32' },
  { name: 'Rivulet_0.2.0-x86_64.apk', browser_download_url: 'https://example.invalid/x64' },
]
assert.equal(pickApk(split, 'arm64-v8a'), 'https://example.invalid/arm64', 'a 64-bit phone takes its own build')
assert.equal(pickApk(split, 'armeabi-v7a'), 'https://example.invalid/arm32', 'and a 32-bit one takes its own')
assert.equal(pickApk(split, 'x86_64'), 'https://example.invalid/x64')
// An architecture nobody recognised must not become a guess: the universal
// build installs on anything, which is the only safe answer.
assert.equal(pickApk(split, ''), 'https://example.invalid/universal', 'an unknown device takes the build that installs anywhere')
// `armeabi-v7a` contains no substring that could match the 64-bit name, but a
// naive `includes` on the other side would — pin both directions.
assert.notEqual(pickApk(split, 'arm64-v8a'), 'https://example.invalid/arm32')
// A release from before the split carries one APK and no suffixes; every
// device must still find it.
const old = [{ name: 'Rivulet_0.2.0.apk', browser_download_url: 'https://example.invalid/only' }]
assert.equal(pickApk(old, 'arm64-v8a'), 'https://example.invalid/only', 'a release with no per-ABI build still updates')
assert.equal(pickApk(old, ''), 'https://example.invalid/only')
// And the version still decides which release build wins, suffix or not.
assert.equal(
  pickApk([
    { name: 'Rivulet.apk', browser_download_url: 'https://example.invalid/stale' },
    { name: 'Rivulet_0.9.0.apk', browser_download_url: 'https://example.invalid/new' },
  ], ''),
  'https://example.invalid/new',
  'an unversioned copy beside a versioned one is not the newer of the two',
)

// A release with no APK is a normal state, not a parse failure — the panel
// falls back to the release page.
assert.equal(parseUpdate({ ...release, assets: [] })?.apk, '')
assert.equal(parseUpdate({ ...release, html_url: undefined })?.url, RELEASES_URL)
assert.equal(parseUpdate({ ...release, body: undefined })?.notes, '')

// Neither should ever reach us from `/releases/latest`, and neither may be
// offered if one does.
assert.equal(parseUpdate({ ...release, draft: true }), null)
assert.equal(parseUpdate({ ...release, prerelease: true }), null)

// Anything without a version is not a release, however well-formed the rest is.
for (const bad of [null, undefined, {}, { tag_name: '' }, { tag_name: 'v' }, 'not json'])
  assert.equal(parseUpdate(bad), null, `rejected: ${JSON.stringify(bad)}`)

// --- What a failed install says ----------------------------------------------
// The updater plugin fetches latest.json from Rust, and reqwest's error arrives
// with its cause chain stripped. Left alone it reads as a broken release rather
// than as a network that was briefly gone, and it tells nobody to try again.

const OPAQUE = 'error sending request for url (https://github.com/baigcoder/rivulet/releases/latest/download/latest.json)'
assert.ok(
  !transportMessage(new Error(OPAQUE)).includes('error sending request'),
  'the stripped transport error must not be shown to the user as-is',
)
assert.match(transportMessage(new Error(OPAQUE)), /Open the release/, 'say where else to get the installer')

// Every other shape of the same nothing.
for (const raw of ['error trying to connect: dns error', 'operation timed out', 'connection refused'])
  assert.notEqual(transportMessage(new Error(raw)), raw, `${raw} is machine noise too`)

// An error that *does* carry a reason keeps it: the manifest genuinely not
// covering this platform is the case the fallback button exists for, and
// flattening it into "check the connection" sends people looking the wrong way.
const REAL = 'The release carries no update for this platform.'
assert.equal(transportMessage(new Error(REAL)), REAL)
assert.equal(transportMessage('a bare string'), 'a bare string')

console.log('check-updates: ok')

// --- The VOD catalog is not loopback work ------------------------------------
// A Premium film list makes the panel hand over its whole VOD catalog, tens of
// megabytes of JSON, which is then paged locally. The desktop does that in a
// second or two; a phone parsing the same payload went past the twenty-second
// ceiling meant for loopback, the prefetch swallowed the abort because the
// section was off screen, and Movies and Series stayed empty and silent. Same
// provider, same build, working on desktop and not on the phone.
const premiumApiTs = readFileSync(new URL('../app/utils/premiumTv.ts', import.meta.url), 'utf8')
assert.match(premiumApiTs, /const CATALOG_TIMEOUT_MS = 90_000/, 'the catalog endpoints get a realistic ceiling')
assert.equal(
  (premiumApiTs.match(/timeoutMs: CATALOG_TIMEOUT_MS/g) ?? []).length,
  5,
  'and all five of them use it — two category lists, two pages, one series detail',
)
assert.match(premiumApiTs, /opts\.timeoutMs \?\? REQUEST_TIMEOUT_MS/, 'everything else keeps the loopback ceiling')

// And a catalog that failed says why. The prefetch runs for a section that is
// off screen by definition, so gating the report on being on screen reported
// nothing at all in the one case that matters.
const premiumStore = readFileSync(new URL('../app/stores/premiumTv.ts', import.meta.url), 'utf8')
assert.match(premiumStore, /vodError\.value = \{ \.\.\.vodError\.value, \[active\]: message\(e\) \}/, 'a failed VOD load is recorded per section')
const browser = readFileSync(new URL('../app/components/premium-tv/PremiumBrowser.vue', import.meta.url), 'utf8')
assert.match(browser, /premium\.vodError\[/, 'and the empty state says it instead of "returned nothing"')
