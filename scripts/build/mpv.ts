/**
 * Put an mpv.exe and an ffmpeg.exe where the Windows build can bundle them.
 *
 * On Linux the player runs whatever mpv and ffmpeg the distro installed.
 * Windows ships neither and has no package manager to lean on, so the app
 * carries its own: `tauri.windows.conf.json` declares them as resources, which
 * puts them next to Rivulet.exe in the installer, and `player_windows.rs` and
 * `lib.rs` resolve them from there. This module is what makes those files exist.
 *
 * They are community builds (the same ones mpv.io points at) — mpv itself
 * publishes no Windows binaries. Both are statically linked, so libass and the
 * D3D11 output come with them and nothing else has to be installed.
 *
 * mpv already contains ffmpeg, but as libraries and with no command line, and
 * the seek previews and the subtitle auto-sync are two processes shelling out
 * to `ffmpeg`. Bundling the CLI is ~105 MB unpacked and is what makes both of
 * them work on a machine that has never heard of ffmpeg — which on Windows is
 * every machine.
 *
 * Bumping them: pick a release from the repo below, then update `BUILD.tag` and
 * each binary's asset name and hash out of that release's sha256.txt. Every
 * download is checked against it. Then run `bun scripts/build/mpv.ts --mirror`
 * once, so builds survive that repo pruning the tag — see `sourcesFor`.
 */
import { execFileSync, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..')

/**
 * https://github.com/zhongfly/mpv-winbuild/releases
 *
 * One release, two binaries, one `sha256.txt`. ffmpeg comes from the same place
 * as mpv because it is the same build of the same libraries — bumping them
 * together is what stops the player and the tools that read what it is playing
 * from drifting apart.
 */
const BUILD = {
  tag: '2026-10-24-a39482e7a5',
  binaries: [
    {
      member: 'mpv.exe',
      asset: 'mpv-x86_64-20261024-git-a39482e7a5.7z',
      sha256: '6ef5b11a52907b8c8a57bc417840a21e111eede53f1b10e7d7a50d9b126967d8',
    },
    /**
     * mpv links ffmpeg statically but exposes no CLI, and the seek previews and
     * the subtitle auto-sync both shell out to one — so without this they are
     * features that only work if the user happened to install ffmpeg himself,
     * which on Windows nobody has.
     */
    {
      member: 'ffmpeg.exe',
      asset: 'ffmpeg-x86_64-git-1d83e7145.7z',
      sha256: 'e365f10e5f4126b5e9f3c9cf5dbe41ff29bd06d625b04d9fe9cd9f7921169955',
    },
  ],
}

/** Where the bundler picks them up from — see tauri.windows.conf.json. */
const DEST_DIR = join(ROOT, 'src-tauri', 'mpv')

/** Kept out of src-tauri so a `cargo clean` doesn't cost another download. */
const CACHE = join(ROOT, '.cache', 'mpv')

/**
 * GPLv2, verbatim, as mpv ships it. Committed rather than fetched: a licence
 *  that only exists if a download succeeds is one a build can silently omit.
 */
const GPL = join(ROOT, 'scripts', 'build', 'mpv-LICENSE.GPL')
const LICENCE = join(DEST_DIR, 'LICENSE.txt')

/**
 * Handing out mpv.exe and ffmpeg.exe is redistributing GPL software, which
 * obliges us to ship the licence with them (§1) and to offer the source they
 * were built from (§3). The upstream archives carry neither — they are the exe
 * and nothing else — so the notice is assembled here, and listed alongside them
 * in tauri.windows.conf.json so the bundler puts the three side by side.
 *
 * The build tag ends in the mpv commit it was made from, which is what makes
 * "the corresponding source" a precise thing rather than a gesture.
 */
function notice() {
  const commit = BUILD.tag.split('-').pop()
  return `Rivulet bundles mpv to play video on Windows, and ffmpeg to read the
audio and the frames of what it is playing.

Both are free software licensed under the GNU General Public License, version 2
or later, reproduced in full below. Rivulet's own code is MIT licensed and is a
separate work: it launches each of them as a child process and talks to them
over an IPC socket or a pipe rather than linking against either.

The binaries shipped beside this file are the community Windows builds

${BUILD.binaries.map(b => `    ${b.asset}`).join('\n')}
    https://github.com/zhongfly/mpv-winbuild/releases/tag/${BUILD.tag}

built from mpv at commit ${commit} (https://github.com/mpv-player/mpv) and from
ffmpeg (https://github.com/FFmpeg/FFmpeg) at the commit its own file name ends
with. They statically link further free-software libraries — libass, libplacebo
and others — each under its own terms; the build recipe at the URL above
enumerates them and is the corresponding source for the combined work.

WRITTEN OFFER
    For three years from the date you received this binary, the Rivulet project
    will give any third party a complete machine-readable copy of the
    corresponding source code for the GPL-licensed parts of it, on a medium
    customarily used for software interchange, for no more than the cost of
    performing the distribution. Request one at
    https://github.com/baigcoder/rivulet/issues.

----------------------------------------------------------------------------

`
}

/**
 * Every 7z reader worth trying, best first. Windows' own `tar` is bsdtar, which
 * reads 7z through libarchive; a Linux box more likely has p7zip. GNU tar comes
 * last because it cannot do this at all and only gets a turn when nothing else
 * exists.
 */
const EXTRACTORS: [string, (archive: string, dir: string, member: string) => string[]][] = [
  ['7z', (a, d, m) => ['x', '-y', `-o${d}`, a, m]],
  ['7za', (a, d, m) => ['x', '-y', `-o${d}`, a, m]],
  ['7zr', (a, d, m) => ['x', '-y', `-o${d}`, a, m]],
  ['bsdtar', (a, d, m) => ['-xf', a, '-C', d, m]],
  ['tar', (a, d, m) => ['-xf', a, '-C', d, m]],
]

function sha256(path: string) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

/**
 * Fetch one asset, and keep trying.
 *
 * A bare `fetch` in a release build is one CDN hiccup away from failing the
 * whole desktop job, and a release has three runners each taking that bet.
 * Backed off rather than immediate, because the thing being waited out is a
 * server having a moment, not a race.
 */
async function download(url: string, to: string) {
  const attempts = 5
  let last: unknown
  for (let attempt = 1; attempt <= attempts; attempt++) {
    try {
      const res = await fetch(url)
      // Gone is gone: a 404 is a release that was pruned, not a server having
      // a moment, and five backed-off retries of it is half a minute of nothing.
      if (res.status === 404)
        throw new Gone(`${url} → HTTP 404`)
      if (!res.ok)
        throw new Error(`${url} → HTTP ${res.status}`)
      writeFileSync(to, new Uint8Array(await res.arrayBuffer()))
      return
    }
    catch (e) {
      last = e
      if (e instanceof Gone)
        break
      if (attempt < attempts)
        await new Promise(r => setTimeout(r, attempt * 2000))
    }
  }
  throw last instanceof Error ? last : new Error(String(last))
}

class Gone extends Error {}

const MIRROR_REPO = 'baigcoder/rivulet'
const MIRROR_TAG = `mpv-${BUILD.tag}`

/**
 * Where an asset can be fetched from, in order. The community repo prunes old
 * releases — a pinned tag goes 404 within weeks, and every fresh Windows build
 * with it — so the same archive is also kept on this project's own releases
 * under `mpv-<tag>`. The checksum is what makes a second address safe:
 * whichever one answers, the bytes are the pinned ones or the build stops.
 * `bun scripts/build/mpv.ts --mirror` puts them there.
 */
function sourcesFor(asset: string) {
  return [
    `https://github.com/zhongfly/mpv-winbuild/releases/download/${BUILD.tag}/${asset}`,
    `https://github.com/${MIRROR_REPO}/releases/download/${MIRROR_TAG}/${asset}`,
  ]
}

/** One binary out of the release, downloaded, checked and unpacked. */
async function ensure(binary: typeof BUILD.binaries[number]): Promise<string> {
  const dest = join(DEST_DIR, binary.member)
  if (existsSync(dest))
    return dest

  mkdirSync(CACHE, { recursive: true })
  const archive = join(CACHE, binary.asset)

  if (!existsSync(archive)) {
    console.log(`→ Fetching ${binary.member} for Windows (${binary.asset}, ~30 MB)`)
    const failures: string[] = []
    for (const url of sourcesFor(binary.asset)) {
      try {
        await download(url, archive)
        break
      }
      catch (e) {
        failures.push(e instanceof Error ? e.message : String(e))
      }
    }
    if (!existsSync(archive)) {
      throw new Error(
        `${binary.asset} is not at any of its addresses:\n  ${failures.join('\n  ')}\n`
        + `  Pick a current release (see the top of scripts/build/mpv.ts) and update BUILD.`,
      )
    }
  }

  // A truncated or tampered download would otherwise surface as a mystery
  // "mpv exited unexpectedly" in the player weeks later.
  const got = sha256(archive)
  if (got !== binary.sha256) {
    rmSync(archive, { force: true })
    throw new Error(
      `${binary.member} download does not match its checksum.\n`
      + `  expected ${binary.sha256}\n`
      + `  got      ${got}\n`
      + `  The file has been deleted; run the build again to retry.`,
    )
  }

  for (const [cmd, args] of EXTRACTORS) {
    const r = spawnSync(cmd, args(archive, DEST_DIR, binary.member), { stdio: 'ignore' })
    if (r.status === 0 && existsSync(dest)) {
      console.log(`✓ Unpacked ${binary.member} with ${cmd}`)
      return dest
    }
  }

  throw new Error(
    `Could not unpack ${archive}.\n`
    + `  It needs a 7z reader: p7zip (\`7z\`) or bsdtar.\n`
    + `  Arch: sudo pacman -S p7zip · Debian: sudo apt install p7zip-full`,
  )
}

/**
 * Ensure src-tauri/mpv/ holds every binary the Windows bundle declares as a
 * resource, downloading and unpacking what is missing. Returns mpv's path, which
 * is the one the build log names. Throws with something actionable if it can't.
 */
export async function ensureMpv(): Promise<string> {
  mkdirSync(DEST_DIR, { recursive: true })
  // Rewritten every run, not just on download: a cached exe from an older tag
  // would otherwise keep a notice pointing at the wrong source.
  writeFileSync(LICENCE, notice() + readFileSync(GPL, 'utf8'))

  const paths: string[] = []
  for (const binary of BUILD.binaries)
    paths.push(await ensure(binary))
  return paths[0]!
}

/** mpv's own version string, for the build log. */
export function mpvVersion(exe: string): string {
  try {
    return execFileSync(exe, ['--version'], { encoding: 'utf8' }).split('\n')[0]?.trim() ?? ''
  }
  catch {
    return '' // not runnable here, which is normal when cross-compiling
  }
}

/**
 * Put the pinned archives on this project's own release `mpv-<tag>`, so a build
 * still finds them once the community repo prunes the tag. Run by a maintainer
 * after bumping BUILD; it needs the GitHub CLI signed in with push access. The
 * archives go up exactly as downloaded — and checksum-checked first.
 */
async function mirror() {
  for (const binary of BUILD.binaries)
    await ensure(binary)
  const files = BUILD.binaries.map(b => join(CACHE, b.asset))
  const exists = spawnSync('gh', ['release', 'view', MIRROR_TAG, '--repo', MIRROR_REPO], { stdio: 'ignore' }).status === 0
  const args = exists
    ? ['release', 'upload', MIRROR_TAG, ...files, '--clobber', '--repo', MIRROR_REPO]
    : ['release', 'create', MIRROR_TAG, ...files, '--repo', MIRROR_REPO, '--title', `mpv ${BUILD.tag}`, '--notes', `Mirror of https://github.com/zhongfly/mpv-winbuild/releases/tag/${BUILD.tag} for reproducible Windows builds. GPLv2+; the licence and source offer ship as mpv/LICENSE.txt in every Rivulet bundle.`]
  if (spawnSync('gh', args, { stdio: 'inherit' }).status !== 0)
    throw new Error('gh could not publish the mirror — is it installed and signed in?')
  return MIRROR_TAG
}

// Also usable on its own: `bun scripts/build/mpv.ts` (or `--mirror`, above)
if (import.meta.main) {
  (process.argv.includes('--mirror') ? mirror() : ensureMpv())
    .then(out => console.log(out))
    .catch((e: Error) => {
      console.error(`\n✗ ${e.message}\n`)
      process.exit(1)
    })
}
