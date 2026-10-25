// Self-check for the loopback servers: `bun scripts/check-security.ts`.
//
// Every server the app runs listens on 127.0.0.1, which keeps the network out
// but not the user's own browser — any page they open can aim a request at
// these ports. This replays what such a page would send and asserts each one
// is refused, then that the app's own requests still get through. It needs the
// app running (`bun run tauri:dev`); with nothing listening it says so and
// passes, like `check:torrents` without `--live`.
//
// node:http rather than fetch: a page's Origin and a rebinding attacker's Host
// are exactly the headers fetch implementations refuse to let a caller set.
import assert from 'node:assert'
import { request } from 'node:http'
import process from 'node:process'

interface Answer { status: number, body: string }

function send(port: number, path: string, headers: Record<string, string> = {}, method = 'GET', body = ''): Promise<Answer> {
  return new Promise((resolve, reject) => {
    const req = request({ host: '127.0.0.1', port, path, method, headers, timeout: 8000 }, res => {
      let text = ''
      res.setEncoding('utf8')
      res.on('data', (c: string) => (text += c))
      res.on('end', () => resolve({ status: res.statusCode ?? 0, body: text }))
    })
    req.on('timeout', () => req.destroy(new Error('timeout')))
    req.on('error', reject)
    req.end(body)
  })
}

const EVIL = { Origin: 'https://evil.example' }
const APP = { Origin: 'http://tauri.localhost' }
const enc = encodeURIComponent

try {
  await send(3030, '/torrents', APP)
}
catch {
  console.log('security: skipped — the app is not running (start it with `bun run tauri:dev`)')
  process.exit(0)
}

// --- A foreign page is refused everywhere --------------------------------------
assert.equal((await send(3030, '/torrents', EVIL)).status, 403, 'engine: a page cannot read the torrent list')
assert.equal((await send(3030, '/torrents/999999/pause', EVIL, 'POST')).status, 403, 'engine: a page cannot send a write')
assert.equal((await send(3030, '/torrents/999999/pause', { Origin: 'null' }, 'POST')).status, 403, 'engine: nor can a sandboxed frame')
assert.equal((await send(3030, '/torrents', { Host: 'evil.example:3030' })).status, 403, 'engine: a rebound name is refused by Host')
for (const target of ['http://127.0.0.1:3030/torrents', 'http://127.0.0.1:3032/api/premium-tv/health', 'http://192.168.0.1/'])
  assert.equal((await send(3031, `/stream?url=${enc(target)}`, EVIL)).status, 403, `proxy: a page cannot relay to ${target}`)
assert.equal((await send(3032, '/api/premium-tv/status')).status, 401, 'premium API: no token, no answer')

// --- Even the app cannot point the proxy home, or the engine at any folder -----
assert.equal((await send(3031, `/stream?url=${enc('http://127.0.0.1:3030/torrents')}`, APP)).status, 403, 'proxy: never onto this machine')
assert.equal((await send(3031, `/stream?url=${enc('file:///C:/Windows/win.ini')}`, APP)).status, 403, 'proxy: never off the web')
const startup = enc('C:\\Users\\x\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup')
assert.equal((await send(3030, `/torrents?list_only=true&output_folder=${startup}`, APP, 'POST', 'magnet:?xt=urn:btih:0000000000000000000000000000000000000000')).status, 403, 'engine: no download into a folder nobody chose')

// --- ...and what the app and native players send still works ------------------
assert.equal((await send(3030, '/torrents', APP)).status, 200, 'engine: the app reads the list')
assert.equal((await send(3030, '/torrents')).status, 200, 'engine: mpv and the Android service send no Origin')
assert.equal((await send(3030, '/torrents/999999/pause', APP, 'POST')).status, 404, 'engine: an app write reaches the engine')
assert.equal((await send(3031, '/health', APP)).status, 200, 'proxy: alive for the app')

console.log('security: ok')
