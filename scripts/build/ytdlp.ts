/**
 * Put a yt-dlp binary where the desktop bundles can ship it.
 *
 * The detail page's trailer playlist used to load through a YouTube <iframe>,
 * which WebKitGTK on Linux cannot run (YouTube error 153 — the "browser not
 * supported" you saw). The player instead resolves the trailer to a direct
 * stream and plays it in a native <video>, via the loopback /youtube-stream
 * proxy, which shells out to yt-dlp. That only works when yt-dlp is present.
 *
 * On Linux and macOS mpv/ffmpeg come from the system, but yt-dlp is not an
 * mpv dependency — a machine that has never heard of it would get a silent
 * 502 and no trailer. So, like Windows already ships its own mpv.exe, every
 * desktop build ships its own yt-dlp: `tauri.{linux,windows,macos}.conf.json`
 * declare it as a bundle resource, which puts it beside the app, and
 * `lib.rs::bundled_ytdlp` resolves it from there.
 *
 * Bumping it: pick a release from the repo below, then update `BUILD.tag` and
 * each binary's asset name and hash out of that release's SHA2-256SUMS. Every
 * download is checked against it. Keep the host-vs-target mapping right:
 * `ensureYtdlp('linux' | 'windows' | 'macos')` selects the binary that runs on
 * the BUILD TARGET, not the machine compiling it.
 */
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { chmodSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..')

/** https://github.com/yt-dlp/yt-dlp/releases */
const BUILD = {
  tag: '2026.08.19',
  // Keyed by the build-target platform. Each is the standalone (PyInstaller)
  // binary, not the zipimport `yt-dlp` that needs a Python interpreter. The
  // member name is what the Rust `bundled_ytdlp` resolver looks for.
  binaries: {
    linux: {
      member: 'yt-dlp',
      asset: 'yt-dlp_linux',
      sha256: '58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a',
    },
    windows: {
      member: 'yt-dlp.exe',
      asset: 'yt-dlp.exe',
      sha256: '66674953fe251b89f4d08c5f0e35e0728679bd67ab3d7d05c0562af101dd3e7a',
    },
    macos: {
      member: 'yt-dlp',
      asset: 'yt-dlp_macos',
      sha256: '0f192b7ec147ab6288885d6351d9ab67367640029b4377576ef46dd79cf7b202',
    },
  },
} as const

type Target = keyof typeof BUILD.binaries

/** Map a Node platform string to one of the target platforms above. */
export function targetPlatform(os: string): Target {
  if (os === 'win32')
    return 'windows'
  if (os === 'darwin')
    return 'macos'
  return 'linux'
}

/** Where the bundler picks the binary up from — see the per-OS conf.json. */
function destDir(): string {
  return join(ROOT, 'src-tauri', 'ytdlp')
}

/** Kept out of src-tauri so a `cargo clean` doesn't cost another download. */
function cacheDir(): string {
  return join(ROOT, '.cache', 'ytdlp')
}

function download(url: string, dest: string): void {
  const r = spawnSync('curl', ['-fSL', '--retry', '3', '-o', dest, url], { stdio: 'inherit' })
  if (r.status !== 0)
    throw new Error(`download failed (curl exited ${r.status ?? 'null'}) for ${url}`)
}

function sha256(path: string): string {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

export function ytdlpVersion(path: string): string {
  const r = spawnSync(path, ['--version'], { encoding: 'utf8', stdio: 'ignore' })
  return r.stdout?.split('\n')[0]?.trim() ?? ''
}

/**
 * Ensure the target platform's yt-dlp binary is in `src-tauri/ytdlp/`.
 * Idempotent: a file already its expected size is left alone, so repeated
 * builds after the first download do not re-fetch 40 MB.
 */
export function ensureYtdlp(target: Target = targetPlatform(process.platform)): string {
  const bin = BUILD.binaries[target]
  const dest = join(destDir(), bin.member)
  if (existsSync(dest)) {
    // Idempotent: re-fetch only if the copy never had its checksum confirmed.
    if (!existsSync(`${dest}.verified`))
      verify(dest, target)
    return dest
  }

  mkdirSync(cacheDir(), { recursive: true })
  mkdirSync(destDir(), { recursive: true })

  const url = `https://github.com/yt-dlp/yt-dlp/releases/download/${BUILD.tag}/${bin.asset}`
  const cached = join(cacheDir(), bin.asset)
  if (!existsSync(cached))
    download(url, cached)

  verify(cached, target)
  // `cp` + chmod rather than rename, so the cache copy stays for other builds.
  const tmp = `${dest}.tmp`
  spawnSync('cp', [cached, tmp], { stdio: 'inherit' })
  chmodSync(tmp, 0o755)
  writeFileSync(`${dest}.verified`, '')
  spawnSync('mv', [tmp, dest], { stdio: 'inherit' })
  return dest
}

function verify(path: string, target: Target): void {
  const bin = BUILD.binaries[target]
  const actual = sha256(path)
  if (actual !== bin.sha256) {
    rmSync(path, { force: true })
    throw new Error(
      `sha256 mismatch for ${bin.asset}\n  expected ${bin.sha256}\n  got      ${actual}`,
    )
  }
}

// Also usable on its own: `bun scripts/build/ytdlp.ts`
if (import.meta.main) {
  const target = process.argv[2] as Target | undefined
  const kind = target || 'host'
  const which = target ?? targetPlatform(process.platform)
  try {
    const path = ensureYtdlp(which)
    console.log(`✓ yt-dlp (${kind}): ${path}`)
  }
  catch (e) {
    console.error(`\n✗ ${e instanceof Error ? e.message : String(e)}\n`)
    process.exit(1)
  }
}
