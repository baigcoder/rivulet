// Self-check for the Premium TV HTTP API: `bun scripts/check-premium-api.ts`.
//
// Two phases, because the useful checks are not the same shape.
//
// The **static** phase always runs and needs nothing: it reads the paths
// `app/utils/premiumTv.ts` actually requests and the route table in
// `src-tauri/src/api/mod.rs`, and asserts they are the same set with the
// same methods. That is the contract that broke silently before — a route
// renamed on one side is a 404 on the other, and nothing but a run of the
// app said so. It also asserts every handler is behind the auth +
// entitlement guard, the redirector included.
//
// The **live** phase runs only if the server answers on loopback, and
// checks what a text search cannot: that an unauthenticated request is
// refused, that a malformed body is a 4xx rather than a panic, and that no
// error body carries a credential or an upstream path. It is skipped, not
// failed, when the app is not running — this script is run from a terminal
// far more often than the app is.

import assert from 'node:assert'
import { globSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const ROOT = fileURLToPath(new URL('..', import.meta.url))
const API = 'http://127.0.0.1:3032'

const CLIENT = readFileSync(`${ROOT}app/utils/premiumTv.ts`, 'utf8')
const ROUTER = readFileSync(`${ROOT}src-tauri/src/api/mod.rs`, 'utf8')
const HANDLERS = readFileSync(`${ROOT}src-tauri/src/api/routes_premium.rs`, 'utf8')

interface CheckResult {
  name: string
  passed: boolean
  detail?: string
}

const results: CheckResult[] = []

function check(name: string, fn: () => Promise<void> | void) {
  return Promise.resolve()
    .then(() => fn())
    .then(() => {
      results.push({ name, passed: true })
    })
    .catch(e => {
      results.push({ name, passed: false, detail: e instanceof Error ? e.message : String(e) })
    })
}

// ── The route table, from both sides ─────────────────────────────

/** `GET /api/premium-tv/channels/:id` → what both sides agree to call it. */
interface Route { method: string, path: string }

/**
 * A path as written in the client, turned into the path a router matches.
 *
 * An interpolation is one of two things and a `?` tells them apart:
 * `${encodeURIComponent(id)}` is a path segment and becomes `:id`, while
 * `${force ? '?force=true' : ''}` is an optional query string and goes,
 * along with the rest of the query — a route matches on path alone.
 */
function normalize(raw: string): string {
  let out = ''
  for (let i = 0; i < raw.length; i++) {
    if (raw[i] === '$' && raw[i + 1] === '{') {
      let depth = 1
      let expr = ''
      for (i += 2; i < raw.length; i++) {
        if (raw[i] === '{')
          depth++
        else if (raw[i] === '}' && --depth === 0)
          break
        expr += raw[i]
      }
      out += expr.includes('?') ? '' : ':id'
      continue
    }
    out += raw[i]
  }
  return out.replace(/\?.*$/, '')
}

/**
 * What the client asks for. Every call goes through `request(method,
 * path)`, so the paths can be read out of the source — but they are
 * template literals with nested interpolation, which needs a scanner and
 * not a regex.
 */
function clientRoutes(): Route[] {
  const out: Route[] = []
  for (const m of CLIENT.matchAll(/request(?:<[^>]*>)?\(\s*'(GET|POST|DELETE)',\s*/g)) {
    let i = m.index! + m[0].length
    const quote = CLIENT[i]
    if (quote !== '\'' && quote !== '`')
      continue
    let raw = ''
    let depth = 0
    for (i++; i < CLIENT.length; i++) {
      const c = CLIENT[i]!
      if (depth === 0 && c === quote)
        break
      if (c === '$' && CLIENT[i + 1] === '{')
        depth++
      else if (depth > 0 && c === '}')
        depth--
      raw += c
    }
    out.push({ method: m[1]!, path: normalize(raw) })
  }
  return out
}

/** What the server registers. */
function serverRoutes(): Route[] {
  const out: Route[] = []
  // `.route("<path>", get(handler))` — the path and the method verb, over
  // a router written across several lines by rustfmt.
  for (const m of ROUTER.matchAll(/\.route\(\s*"([^"]+)",\s*(get|post|delete)\(/g))
    out.push({ method: m[2]!.toUpperCase(), path: m[1]! })
  return out
}

const client = clientRoutes()
const server = serverRoutes()
const key = (r: Route) => `${r.method} ${r.path}`

await check('both parsers found the whole surface', () => {
  // Sixteen premium routes, and one redirector nothing in the client calls.
  assert.ok(client.length >= 16, `only found ${client.length} client calls; the parser is probably broken`)
  assert.ok(server.length >= 17, `only found ${server.length} server routes; the parser is probably broken`)
})

await check('every route the client calls is registered on the server', () => {
  const registered = new Set(server.map(key))
  for (const r of client) {
    assert.ok(
      registered.has(key(r)),
      `client calls ${key(r)}, which the router does not register`,
    )
  }
})

await check('no premium route is registered and never called', () => {
  const called = new Set(client.map(key))
  for (const r of server) {
    if (!r.path.startsWith('/api/premium-tv/'))
      continue
    assert.ok(
      called.has(key(r)),
      `${key(r)} is dead: nothing in the client calls it`,
    )
  }
})

await check('the redirector is the one route no client calls', () => {
  const redirector = server.find(r => r.path.startsWith('/premium-stream/'))
  assert.ok(redirector, 'the redirector must be registered')
  assert.equal(redirector!.method, 'GET', 'a player opens it with GET and nothing else')
  assert.ok(
    !CLIENT.includes('premium-stream'),
    'the client must not build a redirector URL — the server hands it over whole',
  )
})

await check('axum 0.7 path syntax throughout', () => {
  // 0.8 changed `:id` to `{id}`. A bump that misses one of these does not
  // fail to compile, it fails to match.
  for (const r of server) {
    assert.ok(!r.path.includes('{'), `${r.path} uses 0.8 syntax; this crate is on 0.7`)
  }
})

// ── The gate is on every handler ─────────────────────────────────

await check('every handler goes through the auth + entitlement guard', () => {
  // One exception, and it is written down in the handler: `disconnect`
  // takes `require_auth` only, because a lapsed subscriber must still be
  // able to remove their credentials.
  const exempt = new Set(['disconnect'])
  const bodies = HANDLERS.split(/\npub async fn /).slice(1)
  let seen = 0
  for (const body of bodies) {
    const name = body.slice(0, body.indexOf('(')).trim()
    const head = body.slice(0, body.indexOf('\n}\n') + 1 || undefined)
    seen++
    if (exempt.has(name)) {
      assert.ok(head.includes('require_auth(&headers)?'), `${name} must at least require auth`)
      continue
    }
    if (name === 'stream_redirect') {
      // No header to carry a bearer: the signed token in the path is the
      // proof, and the entitlement is re-checked because the token
      // outlives a revocation by up to its TTL.
      assert.ok(head.includes('ensure_premium(&state)?'), 'the redirector must re-check the entitlement')
      assert.ok(head.includes('resolve_redirector_token'), 'and verify the token signature')
      continue
    }
    assert.ok(head.includes('guard(&state, &headers)?'), `${name} is not behind guard()`)
  }
  assert.ok(seen >= 16, `only inspected ${seen} handlers; the parser is probably broken`)
})

await check('the guard is auth and entitlement, in that order', () => {
  const guard = HANDLERS.slice(HANDLERS.indexOf('fn guard('))
  const body = guard.slice(0, guard.indexOf('\n}'))
  const auth = body.indexOf('require_auth')
  const gate = body.indexOf('ensure_premium')
  assert.ok(auth > 0 && gate > auth, 'an unauthenticated caller must not learn whether it is premium')
})

await check('the entitlement defaults to denied', () => {
  const gate = HANDLERS.slice(HANDLERS.indexOf('fn ensure_premium('))
  const body = gate.slice(0, gate.indexOf('\n}'))
  assert.ok(/PremiumRequired|premium_required/i.test(body), 'the refusal must be the premium error')
})

// ── Credentials never leave the Rust side ────────────────────────

/**
 * mpv's log tail is the most useful thing a bug report carries and the
 * most dangerous: a live stream resolves to
 * `http://host:8080/live/<username>/<password>/1234.m3u8`, so the line
 * that explains a failure is also the line that leaks the account. Every
 * platform reads that tail in its own file, and only one of them compiles
 * on any given machine — which is exactly the shape of bug that ships.
 */
await check('every platform redacts the mpv log tail before returning it', () => {
  // X11 and macOS share `player_socket`; Win32 reads the file itself.
  for (const file of ['src-tauri/src/player_socket.rs', 'src-tauri/src/player_windows.rs']) {
    const rust = readFileSync(`${ROOT}${file}`, 'utf8')
    assert.ok(
      rust.includes('log_tail'),
      `${file} no longer builds a log tail — move this check to whatever replaced it`,
    )
    assert.ok(
      rust.includes('log_redact::redact'),
      `${file} returns the tail unredacted; a stream URL's path is the account's password`,
    )
  }
})

/**
 * The frontend restarts the player when its `src` changes, so two
 * authorizations for one channel must not be the same string. `iat`/`exp`
 * are whole seconds, so without a nonce two mints inside the same second
 * produced a byte-identical URL and the watcher saw no change at all.
 */
await check('a minted stream token is unique per mint', () => {
  const auth = readFileSync(`${ROOT}src-tauri/src/api/auth.rs`, 'utf8')
  assert.ok(/pub jti: String/.test(auth), 'StreamClaims must carry a per-mint nonce')
  const mint = auth.slice(auth.indexOf('fn mint_stream_token'))
  const body = mint.slice(0, mint.indexOf('\n}'))
  assert.ok(/fill_bytes|random|thread_rng/.test(body), 'the nonce must be random, not derived from the clock')
  assert.ok(/jti:/.test(body), 'and it must actually be put on the claims')
})

/** Every Rust file in the premium module and its HTTP API. */
const RUST_SOURCES = [
  ...globSync(`${ROOT}src-tauri/src/premium/*.rs`),
  ...globSync(`${ROOT}src-tauri/src/api/*.rs`),
]

await check('an error that reaches stderr cannot carry a credential', () => {
  // Every `eprintln!` in the premium module prints a `PremiumError` (or an
  // `ApiError::Internal` built from one) through Display. The two variants
  // that can hold an upstream error string also hold that string's URL, and
  // an Xtream URL's path *is* the username and password — so their Display
  // arms must throw the payload away rather than print it.
  const errs = readFileSync(`${ROOT}src-tauri/src/premium/errors.rs`, 'utf8')
  const display = errs.slice(errs.indexOf('impl fmt::Display for PremiumError'))
  for (const variant of ['ServerError', 'Network']) {
    const arm = display.slice(display.indexOf(`PremiumError::${variant}(`))
    assert.ok(
      arm.startsWith(`PremiumError::${variant}(_)`),
      `${variant}'s Display binds its inner string; an upstream error text carries the request URL`,
    )
  }
  // And nothing may log the Debug form, which keeps what Display drops.
  for (const file of RUST_SOURCES) {
    const rust = readFileSync(file, 'utf8')
    const debugLog = rust.match(/e?println!\([^)]*\{[a-z_]+:\?\}/)
    assert.equal(debugLog, null, `${file} logs a Debug-formatted value: ${debugLog?.[0]}`)
  }
})

// ── Live phase ───────────────────────────────────────────────────

async function reachable(): Promise<boolean> {
  try {
    await fetch(`${API}/api/premium-tv/status`, { signal: AbortSignal.timeout(1200) })
    return true
  }
  catch {
    return false
  }
}

function get(path: string, headers: Record<string, string> = {}) {
  return fetch(`${API}${path}`, { method: 'GET', headers })
}
function post(path: string, body?: unknown, headers: Record<string, string> = {}) {
  return fetch(`${API}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...headers },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
}

/**
 * A body each POST route will accept, so the only ground left to refuse on
 * is the missing token. Axum runs its extractors before the handler body,
 * so a route with a required field answers 422 to `{}` — which says
 * nothing about auth, and is what this check is not about.
 */
const BODIES: Record<string, unknown> = {
  '/api/premium-tv/epg/now-next': { channelIds: [] },
  '/api/premium-tv/recent': { channelId: 'nonexistent' },
}

const live = await reachable()

if (live) {
  await check('every route refuses an unauthenticated caller with 401', async () => {
    for (const r of server) {
      if (r.path.startsWith('/premium-stream/'))
        continue
      const path = r.path.replace(':id', 'nonexistent')
      const resp = r.method === 'GET' ? await get(path) : await post(path, BODIES[path] ?? {})
      assert.equal(resp.status, 401, `${key(r)} answered ${resp.status}`)
    }
  })

  await check('an invalid bearer is 401, not 500', async () => {
    const resp = await get('/api/premium-tv/status', { Authorization: 'Bearer not-a-real-token' })
    assert.equal(resp.status, 401)
  })

  await check('the redirector refuses a forged token', async () => {
    const resp = await get('/premium-stream/not-a-token', {})
    assert.ok(resp.status === 401 || resp.status === 403 || resp.status === 404, `got ${resp.status}`)
    const body = await resp.text()
    assert.ok(!/http/i.test(body), 'and must not name an upstream in the refusal')
  })

  await check('a malformed body is a 4xx', async () => {
    const resp = await fetch(`${API}/api/premium-tv/connect`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: 'this is not json',
    })
    assert.ok(resp.status >= 400 && resp.status < 500, `expected 4xx, got ${resp.status}`)
  })

  await check('no error body carries a credential or an upstream path', async () => {
    for (const r of server) {
      if (r.path.startsWith('/premium-stream/'))
        continue
      const path = r.path.replace(':id', 'nonexistent')
      const resp = r.method === 'GET' ? await get(path) : await post(path, BODIES[path] ?? {})
      const body = (await resp.text()).toLowerCase()
      for (const leak of ['password', 'passwd', 'username=', 'player_api', 'live/', 'xmltv']) {
        assert.ok(!body.includes(leak), `${key(r)} leaked '${leak}'`)
      }
    }
  })
}

// ── Report ───────────────────────────────────────────────────────

const passed = results.filter(r => r.passed).length
const failed = results.length - passed
for (const r of results) {
  console.log(`${r.passed ? '✓' : '✗'} ${r.name}${r.passed ? '' : ` — ${r.detail}`}`)
}
if (!live)
  console.log('\n· live checks skipped: nothing is listening on 127.0.0.1:3032 (start the app to run them)')
console.log(`\n${passed} passed, ${failed} failed`)
if (failed === 0)
  console.log('premium api: ok')
process.exit(failed > 0 ? 1 : 0)
