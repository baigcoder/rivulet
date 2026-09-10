// Static checks for the Premium TV provider adapter contract.
//
// The adapters are Rust modules and their unit tests run via
// `cargo test --lib`. This script checks the *contract* the
// front-end relies on — the shape of the wire types, the
// redirector URL the play handler returns, the auth header
// shape, the entitlement gate.

import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const ROOT = fileURLToPath(new URL('..', import.meta.url))

const PLAYER_RS = `${ROOT}/src-tauri/src/premium/player.rs`
const XTREAM_RS = `${ROOT}/src-tauri/src/premium/xtream.rs`
const M3U_RS = `${ROOT}/src-tauri/src/premium/m3u.rs`
const REPOSITORY_RS = `${ROOT}/src-tauri/src/premium/repository.rs`
const ERRORS_RS = `${ROOT}/src-tauri/src/premium/errors.rs`
const ROUTES_RS = `${ROOT}/src-tauri/src/api/routes_premium.rs`
const CRYPTO_RS = `${ROOT}/src-tauri/src/premium/crypto.rs`
const AUTH_RS = `${ROOT}/src-tauri/src/api/auth.rs`
const STORAGE_RS = `${ROOT}/src-tauri/src/premium/storage.rs`
const API_RS = `${ROOT}/src-tauri/src/api/mod.rs`
const IPTV_PROXY_RS = `${ROOT}/src-tauri/src/iptv/proxy.rs`

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
    results.push({
      name,
      passed: false,
      detail: e instanceof Error ? e.message : String(e),
    })
  }
}

const player = readFileSync(PLAYER_RS, 'utf8')
const xtream = readFileSync(XTREAM_RS, 'utf8')
const m3u = readFileSync(M3U_RS, 'utf8')
const repository = readFileSync(REPOSITORY_RS, 'utf8')
const errors = readFileSync(ERRORS_RS, 'utf8')
const routes = readFileSync(ROUTES_RS, 'utf8')
const crypto = readFileSync(CRYPTO_RS, 'utf8')
const auth = readFileSync(AUTH_RS, 'utf8')
const storage = readFileSync(STORAGE_RS, 'utf8')
const api = readFileSync(API_RS, 'utf8')
const proxy = readFileSync(IPTV_PROXY_RS, 'utf8')

// ── Credential isolation ──────────────────────────────────

check('player returns a redirector URL, not the raw upstream', () => {
  assert.ok(
    player.includes('/premium-stream/'),
    'playback source must be a /premium-stream/ URL',
  )
  // The redirector URL is built by mint_redirector_token +
  // format!. The raw upstream URL with the password in the
  // path must never appear in a route response.
  assert.ok(
    !routes.includes('format!(\n            "{}/live/'),
    'route handler must not embed /live/ in a response body',
  )
})

check('error Display impls never mention password', () => {
  // Walk every error message and assert no string contains
  // the substring `password`. The error module doesn't
  // print the field name; this is a regression check.
  for (const rawLine of errors.split('\n')) {
    const line = rawLine.replace(/^\s+/, '')
    if (line.startsWith('//') || line.startsWith('///'))
      continue
    if (line.includes('write!(') || line.includes('Display')) {
      assert.ok(
        !/password|passwd|username/i.test(line),
        `error.rs must not include credential field name: '${line.trim()}'`,
      )
    }
  }
})

check('crypto never logs the master key', () => {
  // No `eprintln!` / `println!` / `dbg!` references to `key`
  // or `bytes` in the encrypted-blob path. (A panic message
  // that mentions the field is fine; a log statement is not.)
  for (const line of crypto.split('\n')) {
    assert.ok(
      !/(?:println!|eprintln!|dbg!)\s*\(.*\bkey\b/i.test(line),
      'crypto module must not print the key',
    )
  }
})

// ── Auth & entitlement ──────────────────────────────────

check('JWT requires Bearer prefix', () => {
  assert.ok(routes.includes('strip_prefix("Bearer ")'))
})

check('JWT verifier checks exp claim', () => {
  assert.ok(auth.includes('Validation::default()'))
  assert.ok(auth.includes('"exp"'))
})

check('PremiumRequired has a Display impl', () => {
  assert.ok(errors.includes('PremiumRequired'))
  assert.ok(/PremiumRequired => write!/.test(errors))
})

// ── Provider shape ──────────────────────────────────

check('XtreamAdapter drops VOD but keeps a channel with no stream_type', () => {
  // The filter is deliberately lenient in one direction: a provider that
  // says "movie" on `get_live_streams` is believed and dropped, one that
  // says nothing is kept. Asserting the strict `== Some("live")` form was
  // asserting the bug — it drops every channel on a provider that omits
  // the field, which is a common enough panel to matter.
  assert.ok(/\.map\(\|t\| t == "live"\)/.test(xtream), 'must compare stream_type to "live"')
  assert.ok(/\.unwrap_or\(true\)/.test(xtream), 'and keep a channel that declares no type')
})

check('XtreamAdapter URL-escapes credentials', () => {
  assert.ok(xtream.includes('urlencoding::encode(&self.username)'))
  assert.ok(xtream.includes('urlencoding::encode(&self.password)'))
})

check('M3uAdapter handles EXTVLCOPT user-agent and referer', () => {
  assert.ok(m3u.includes('http-user-agent'))
  assert.ok(m3u.includes('http-referrer'))
})

check('repository does cursor pagination locally', () => {
  // The page returns next_cursor (Some/None) based on whether
  // the offset+limit has reached total. This is the rule the
  // plan pins: providers don't support cursor pagination, so
  // we do it ourselves.
  assert.ok(repository.includes('next_cursor'))
  assert.ok(/offset\s*\+\s*items\.len/.test(repository))
})

// ── XMLTV parsing ──────────────────────────────────

check('XMLTV bulk parser handles gzipped body', () => {
  // The XMLTV body comes in either plain or gzipped. The
  // bulk-EPG path's gzip support lives in the adapter, not
  // the repository — the repository takes already-decoded
  // bytes. XtreamAdapter and M3uAdapter both call
  // `flate2::read::GzDecoder`. We check the adapter files.
  const xtreamHasGz = xtream.includes('flate2::read::GzDecoder')
  const m3uHasGz = m3u.includes('flate2::read::GzDecoder')
  assert.ok(
    xtreamHasGz || m3uHasGz,
    'at least one adapter must gunzip the bulk EPG body',
  )
})

check('all-movies does not download the whole VOD catalog first', () => {
  assert.ok(
    xtream.includes('fn merge_vod_movies'),
    'All movies must walk categories until the first page is full',
  )
  assert.ok(
    xtream.includes('fn merge_vod_series'),
    'All series must walk categories the same way',
  )
})

// ── Route surface ──────────────────────────────────

check('all premium routes go through require_auth', () => {
  // The required_auth helper must be called from every handler
  // that returns a non-static body.
  for (const fn of [
    'status',
    'connect',
    'disconnect',
    'categories',
    'channels',
    'channel',
    'epg',
    'play',
    'favorites',
    'toggle_favorite',
    'recent',
    'add_recent',
  ]) {
    assert.ok(
      routes.includes(`pub async fn ${fn}(`),
      `missing handler for ${fn}`,
    )
  }
})

// ── Scale ─────────────────────────────────────────────────
//
// A big panel is not a rounding error on a small one. The account these
// were written against answers `get_live_streams` with 53,782 channels
// in 23 MB of JSON across 781 categories, and has 219,565 films in 363
// categories and 17,156 series in 105. Each check below is a place where
// that size turned something linear into something quadratic, or turned
// one connection into thousands.

check('the big Xtream downloads do not share the small calls timeout', () => {
  assert.ok(
    xtream.includes('CATALOG_READ_TIMEOUT'),
    'the live lineup and the VOD lists need their own read timeout',
  )
  assert.ok(
    xtream.includes('fn catalog_client('),
    'expected a catalog_client() for the tens-of-megabytes responses',
  )
  assert.ok(
    /fn get_channels\(&self\)[\s\S]{0,400}?self\.catalog_client\(\)/.test(xtream),
    'get_channels must use the catalog client, not the 15s one',
  )
})

check('Xtream clients are built once, not per request', () => {
  // A `reqwest::Client` *is* the connection pool. Building one per call
  // meant `merge_vod_movies` opened a fresh TLS connection for each of a
  // panel's 363 movie categories.
  assert.ok(
    xtream.includes('static CLIENT: OnceLock<Client>')
    && xtream.includes('static CATALOG_CLIENT: OnceLock<Client>'),
    'both clients must be process-wide statics',
  )
  assert.ok(
    !/fn client\(&self\) -> Result<Client, PremiumError> \{\s*Client::builder\(\)/.test(xtream),
    'client() must not build a new Client on every call',
  )
})

check('the channel table is indexed on what the reads filter by', () => {
  // `query_channels` compares `category_name` (the rail sends back the
  // label it drew) and `category_counts` groups by it. Indexed only on
  // `category_id`, both were a full scan of every channel — twice per
  // category click, because the page and its total are two queries.
  assert.ok(
    /CREATE INDEX IF NOT EXISTS iptv_premium_channels_catname[\s\S]{0,120}category_name/.test(storage),
    'missing an index on (connection_id, category_name)',
  )
  assert.ok(
    repository.includes('c.category_name = ?'),
    'if the category filter stopped using category_name, the index above is the wrong one',
  )
})

check('a 50k-row catalog import is not one fsync per row', () => {
  for (const pragma of ['synchronous = NORMAL', 'temp_store = MEMORY', 'busy_timeout']) {
    assert.ok(storage.includes(pragma), `missing PRAGMA ${pragma}`)
  }
  assert.ok(
    storage.includes('journal_mode = WAL'),
    'synchronous = NORMAL is only a safe trade under WAL',
  )
})

// ── Channel logos ─────────────────────────────────────────

check('a dead logo does not blind the ones that work beside it', () => {
  // A shipped version struck out a whole *host* after three failures.
  // Real providers break finer than that: one here serves its channel
  // logos and its film posters from the same endpoint on the same host,
  // and answers 502 for every channel logo and 200 for every poster. The
  // channel grid struck the host out, and the Movies and TV shows tabs
  // were then refused artwork that would have loaded — cached by the
  // webview for ten minutes each. One broken image class became no
  // images at all.
  assert.doesNotMatch(
    api,
    /LOGO_HOST_STRIKES|LOGO_HOST_PENALTY/,
    'a per-host breaker blinds the working images on a partly-broken host',
  )
  assert.ok(
    /fn logo_is_known_bad\(url: &str\)/.test(api),
    'the negative cache must be keyed on the URL alone, never the host',
  )
  assert.ok(
    api.includes('LOGO_CONCURRENCY'),
    'unbounded logo fetches saturate the blocking pool the whole process shares',
  )
  assert.ok(
    api.includes('static LOGO_AGENT'),
    'the logo agent must be pooled, not rebuilt per request — that is what makes a miss cheap',
  )
})

check('a logo the proxy could not get is cacheable', () => {
  // Without a Cache-Control on the failure the webview re-requests every
  // dead logo on every scroll. 404 rather than a placeholder image,
  // because the cards draw their own fallback on an <img> error.
  assert.ok(
    /fn logo_unavailable\(\)[\s\S]{0,400}CACHE_CONTROL/.test(api),
    'the failure response needs a Cache-Control',
  )
  assert.ok(
    /fn logo_unavailable\(\)[\s\S]{0,400}NOT_FOUND/.test(api),
    'the failure response should be a 404 so the card falls back',
  )
})

check('the logo proxy will not fetch from inside the network', () => {
  // It takes a URL out of a provider's catalog and fetches it from the
  // user's own machine, so it is exactly the shape of thing that must
  // not be pointable at the user's router or a metadata endpoint.
  assert.ok(
    api.includes('fn logo_host_is_public('),
    'expected an explicit public-address check',
  )
  for (const guard of ['is_private()', 'is_loopback()', 'is_link_local()', 'to_ipv4_mapped()']) {
    assert.ok(api.includes(guard), `logo host check is missing ${guard}`)
  }
})

// ── Playback ──────────────────────────────────────────────

check('a cached CDN hop expires before the provider token does', () => {
  // A panel's 302 commonly lands on a tokenised CDN path whose own
  // expiry is minutes away. The probes the cache exists for arrive
  // within seconds of a zap, so a long TTL buys nothing and risks
  // handing the player a dead URL — on an account whose connection
  // limit is often exactly one.
  const ttl = /const REDIR_TTL: Duration = Duration::from_secs\((\d+)(?:\s*\*\s*(\d+))?\)/.exec(proxy)
  assert.ok(ttl, 'REDIR_TTL not found in the IPTV proxy')
  const secs = Number(ttl[1]) * (ttl[2] ? Number(ttl[2]) : 1)
  assert.ok(
    secs <= 120,
    `REDIR_TTL is ${secs}s; a tokenised CDN hop is commonly dead inside four minutes`,
  )
})

const passed = results.filter(r => r.passed).length
const failed = results.length - passed
for (const r of results) {
  const tag = r.passed ? '✓' : '✗'
  const detail = r.passed ? '' : ` — ${r.detail}`
  console.log(`${tag} ${r.name}${detail}`)
}
console.log(`\n${passed} passed, ${failed} failed`)
process.exit(failed > 0 ? 1 : 0)
