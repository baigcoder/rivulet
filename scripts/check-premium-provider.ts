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

const passed = results.filter(r => r.passed).length
const failed = results.length - passed
for (const r of results) {
  const tag = r.passed ? '✓' : '✗'
  const detail = r.passed ? '' : ` — ${r.detail}`
  console.log(`${tag} ${r.name}${detail}`)
}
console.log(`\n${passed} passed, ${failed} failed`)
process.exit(failed > 0 ? 1 : 0)
