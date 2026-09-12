import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { deviceCodecs, hasNativePlayer, hasVideoOverlay, uhdPlayable, videoEngine, vlcEngine } from '../app/utils/htmlvideo'
import { nearestFrame, walkOrder } from '../app/utils/thumbs'
// Self-check for the <video> player backend: `bun scripts/check-player.ts`.
//
// The shim answers mpv's command/property protocol so one component drives
// either backend (app/utils/htmlvideo.ts). What's worth pinning down is the
// translation itself — a volume that's out by 100x or a `sid` that doesn't
// stick shows up as a silent film or missing subtitles, neither of which says
// which side got it wrong.
import './i18n-stub'

/** Just the surface `videoEngine` touches — a DOM would be a dependency for six properties. */
function fakeVideo() {
  const listeners: Record<string, (() => void)[]> = {}
  return {
    paused: true,
    currentTime: 0,
    duration: Number.NaN,
    volume: 1,
    muted: false,
    playbackRate: 1,
    readyState: 4,
    error: null as { code: number } | null,
    src: '',
    style: { objectFit: 'contain' },
    buffered: { length: 1, start: () => 0, end: () => 90 },
    seekable: { length: 1, start: () => 0, end: () => 95 },
    play() {
      this.paused = false
      return Promise.resolve()
    },
    pause() {
      this.paused = true
    },
    load() {},
    removeAttribute(name: string) {
      if (name === 'src')
        this.src = ''
    },
    addEventListener(name: string, fn: () => void) {
      (listeners[name] ??= []).push(fn)
    },
    /** Stand-in for the element firing one of its own events. */
    emit(name: string) {
      listeners[name]?.forEach(fn => fn())
    },
  }
}

const video = fakeVideo()

const player = videoEngine(video as any)

// Out here there is no Tauri at all, which is the same answer a browser gives
// and the reason `bun run dev` gets a working player.
assert.equal(hasNativePlayer(), false)
assert.equal(vlcEngine(), null, 'and no Android bridge, so no libVLC either')
// Nor a surface in front of the page: both platform questions have to answer no
// off Tauri, or the browser build punches holes for a window that isn't there.
assert.equal(hasVideoOverlay(), false)

// Nothing has been started, so nothing is running and nothing has failed.
assert.deepEqual(player.status(), { running: false, log_tail: null })

await player.start('http://127.0.0.1:3030/torrents/1/stream/0')
assert.equal(video.src, 'http://127.0.0.1:3030/torrents/1/stream/0')
assert.equal(video.paused, false, 'a started file plays')
assert.equal(player.status().running, true)

// --- Properties, as the poll reads them ---------------------------------------
video.currentTime = 42
video.duration = 5400
const p = player.props(['pause', 'time-pos', 'duration', 'volume', 'demuxer-cache-time', 'paused-for-cache'])
assert.equal(p.pause, false)
assert.equal(p['time-pos'], 42)
assert.equal(p.duration, 5400)
assert.equal(p.volume, 100, 'mpv counts volume to 100, the element to 1')
assert.equal(p['demuxer-cache-time'], 90, 'absolute, like mpv reports it — not a length')
assert.equal(p['paused-for-cache'], false)
assert.equal(
  player.props(['cache-buffering-state'])['cache-buffering-state'],
  100,
  '48s ahead of a 1s Direct cache is full',
)

video.buffered = { length: 1, start: () => 0, end: () => 0.5 }
video.currentTime = 0
assert.equal(
  player.props(['cache-buffering-state'])['cache-buffering-state'],
  50,
  'half of the 1s Direct startup buffer',
)
video.buffered = { length: 1, start: () => 0, end: () => 90 }
video.currentTime = 42

// A duration the element hasn't worked out yet must read 0, not NaN: the seek
// bar divides by it.
video.duration = Number.NaN
assert.equal(player.props(['duration']).duration, 0)
video.duration = 5400

// Anything the shim can't produce is simply absent, exactly as mpv leaves a
// property it has no answer for.
assert.deepEqual(player.props(['mouse-pos']), {})

// A Dolby/DTS track the device can't decode plays silently and raises nothing,
// so zero decoded bytes is the warning. No counter at all must stay quiet — the
// alternative is telling every Firefox user their sound is broken.
assert.equal(player.props(['silent']).silent, false, 'no counter, no claim')
;(video as any).webkitAudioDecodedByteCount = 0
assert.equal(player.props(['silent']).silent, true)
;(video as any).webkitAudioDecodedByteCount = 4096
assert.equal(player.props(['silent']).silent, false)
delete (video as any).webkitAudioDecodedByteCount

// Buffered ranges that don't reach the playhead are somewhere else in the file.
video.currentTime = 600
assert.equal(player.props(['demuxer-cache-time'])['demuxer-cache-time'], 0)
video.currentTime = 42

// Starved of data mid-play is a stall; the same readyState while paused is not.
video.readyState = 1
assert.equal(player.props(['paused-for-cache'])['paused-for-cache'], true)
video.readyState = 2
assert.equal(player.props(['paused-for-cache'])['paused-for-cache'], false, 'HAVE_CURRENT_DATA is enough to start Direct play')
video.readyState = 4

// --- Commands, as the controls send them ---------------------------------------
player.command(['set_property', 'pause', true])
assert.equal(video.paused, true)
player.command(['set_property', 'time-pos', 120])
assert.equal(video.currentTime, 120)
player.command(['set_property', 'volume', 40])
assert.equal(video.volume, 0.4)
player.command(['set_property', 'volume', 250])
assert.equal(video.volume, 1, 'clamped, or the element throws and the volume sticks')
player.command(['set_property', 'speed', 1.5])
assert.equal(video.playbackRate, 1.5)
player.command(['set_property', 'mute', true])
assert.equal(video.muted, true)

player.command(['set_property', 'video-scale', 'cover'])
assert.equal(video.style.objectFit, 'cover', 'Center is object-fit: cover')
player.command(['set_property', 'video-scale', 'fill'])
assert.equal(video.style.objectFit, 'fill', 'Stretch is object-fit: fill')
player.command(['set_property', 'video-scale', 'contain'])
assert.equal(video.style.objectFit, 'contain', 'Fit is object-fit: contain')

// Live "go live": percent-seek is the end of `seekable`, not duration * pct.
// A live window's duration is Infinity and the element would throw.
video.currentTime = 10
player.command(['seek', 100, 'absolute-percent'])
assert.equal(video.currentTime, 95, 'percent-seek on live is the seekable end')
player.command(['seek', 5, 'relative'])
assert.equal(video.currentTime, 100, 'relative seek is a delta')
player.command(['seek', 12, 'absolute'])
assert.equal(video.currentTime, 12)

// Unknown commands are ignored rather than thrown: `keybind` has no window to
// bind on and `show-text` is drawn by the page.
assert.doesNotThrow(() => player.command(['keybind', 'MBTN_LEFT', 'cycle pause']))
assert.doesNotThrow(() => player.command(['show-text', 'hello', 1200, 0]))

// --- Subtitles ------------------------------------------------------------------
// `sub-add` adds *and* selects, which is what mpv does and what the menu counts
// on: picking a language has to switch to it in one step.
// Ids start above 1000 so they can never collide with a track libVLC found
// inside the file, which the other backend merges into the same menu.
const first = player.command(['sub-add', 'https://subs/a.srt', 'cached', 'English', 'eng'])
assert.equal(first, 1001)
assert.equal(player.props(['sid']).sid, 1001)
player.command(['sub-add', 'https://subs/b.srt', 'cached', 'German', 'ger'])
assert.equal(player.props(['sid']).sid, 1002)

// The same file again re-selects it instead of stacking a duplicate track —
// which is what `cached` means, and what re-picking a language does.
assert.equal(player.command(['sub-add', 'https://subs/a.srt', 'cached', 'English', 'eng']), 1001)
const tracks = player.props(['track-list'])['track-list'] as { id: number, external?: boolean }[]
assert.equal(tracks.length, 2)
assert.ok(tracks.every(t => t.external), 'nothing here is muxed into the file')

player.command(['set_property', 'sid', 'no'])
assert.equal(player.props(['sid']).sid, 'no', 'subtitles off stays off')

// --- Ending, and failing ---------------------------------------------------------
video.emit('ended')
assert.deepEqual(player.status(), { running: false, log_tail: null }, 'playing out is not a failure')

await player.start('http://127.0.0.1:3030/torrents/1/stream/0')
video.error = { code: 4 }
video.emit('error')
assert.equal(player.status().running, false)
assert.match(player.status().log_tail ?? '', /x264/, 'the decode errors say what to do about it')

// Tearing down clears the source, and an engine that reports that as an error
// must not overwrite the real reason playback stopped.
player.stop()
assert.equal(video.src, '', 'the reader on the engine is let go of')
video.emit('error')
assert.equal(player.status().running, false)

// --- libVLC, on Android --------------------------------------------------------
// The same protocol over a @JavascriptInterface instead of a DOM element, so
// most of it is Kotlin's problem (VlcPlayer.kt) and untestable from here. What is
// worth pinning down is the half Kotlin is deliberately *not* told about:
// external subtitles belong to the page, so their ids have to survive the round
// trip and can never be confused with a track found inside the file.
const sent: unknown[][] = []
const bridge = {
  state: {
    'sid': 'no' as unknown,
    'aid': 1 as unknown,
    'track-list': [
      { id: 1, type: 'audio', title: 'EAC3 5.1' },
      { id: 2, type: 'sub', lang: 'eng' },
    ] as unknown,
  } as Record<string, unknown>,
  start() {},
  stop() {},
  command(json: string) {
    const cmd = JSON.parse(json) as unknown[]
    sent.push(cmd)
    if (cmd[0] === 'set_property')
      this.state[String(cmd[1])] = cmd[2]
    return 'null'
  },
  props(json: string) {
    const out: Record<string, unknown> = {}
    for (const name of JSON.parse(json) as string[]) {
      if (name in this.state)
        out[name] = this.state[name]
    }
    return JSON.stringify(out)
  },
  status: () => JSON.stringify({ running: true, log_tail: null }),
  codecs: () => JSON.stringify(['audio/eac3', 'video/hevc']),
}
;(globalThis as any).RivuletPlayer = bridge

// --- 4K on a phone ----------------------------------------------------------------
// `codecs()` says HEVC on every phone, because Google's software decoder ships
// with the platform — and a phone CPU decoding 4K in software plays a frozen
// picture over audio that keeps going. `videoCaps()` answers the question that
// actually matters: a *hardware* decoder that takes 3840×2160, and 10-bit.
assert.equal(uhdPlayable('Film.2019.2160p.x265'), null, 'an APK from before videoCaps() is "unknown", never "unplayable"')
// The decoder walk runs on a background thread, because a bridge call blocks the
// page and every bridge call queued behind it — the player's own start included.
// Until it finishes the answer is "", which is still "unknown", and not cached.
;(bridge as { videoCaps?: () => string }).videoCaps = () => ''
assert.equal(uhdPlayable('Film.2019.2160p.x265'), null, 'a device still working it out is "unknown" too')
;(bridge as { videoCaps?: () => string }).videoCaps = () => JSON.stringify({ 'video/hevc': { uhd: true, uhd10: false } })
assert.equal(uhdPlayable('Film.2019.1080p.x265'), true, 'under 4K is not this question')
assert.equal(uhdPlayable('Film.2019.2160p.x265'), true, 'a hardware HEVC decoder at 4K plays 8-bit')
assert.equal(uhdPlayable('Film 2019 4K'), true, 'a UHD name with no codec is HEVC')
assert.equal(uhdPlayable('Film.2019.2160p.HDR.x265'), false, 'HDR is 10-bit, which this decoder lacks')
assert.equal(uhdPlayable('Film.2019.2160p.DV.HEVC'), false, 'and so is Dolby Vision')
assert.equal(uhdPlayable('Film.2019.2160p.HDRip.x265'), true, 'HDRip is a web rip, not HDR')
assert.equal(uhdPlayable('Film.2019.2160p.AV1'), false, 'no hardware AV1 decoder takes 4K on this device')

const vlc = vlcEngine()!
assert.ok(vlc, 'the bridge being there is what decides, not the platform')

// A track inside the file is libVLC's to select, and goes through untouched.
vlc.command(['set_property', 'aid', 1])
assert.deepEqual(sent.at(-1), ['set_property', 'aid', 1])

// A downloaded one is not: the page draws it, so libVLC's own text renderer
// has to go off or the two would draw over each other.
assert.equal(vlc.command(['sub-add', 'https://subs/a.srt', 'cached', 'English', 'eng']), 1001)
assert.deepEqual(sent.at(-1), ['set_property', 'sid', 'no'])
assert.equal(vlc.props(['sid']).sid, 1001, 'which libVLC would otherwise report as off')

const merged = vlc.props(['track-list'])['track-list'] as { id: number }[]
assert.deepEqual(merged.map(t => t.id), [1, 2, 1001], 'one menu, and no id used twice')

// Picking one of the file's own hands selection back to libVLC.
vlc.command(['set_property', 'sid', 2])
assert.deepEqual(sent.at(-1), ['set_property', 'sid', 2])
assert.equal(vlc.props(['sid']).sid, 2)

// What `isAwkward` asks before demoting a release for a codec this device may
// well have — the whole reason a TV box stops being handed the x264 copy.
assert.ok(deviceCodecs()?.has('audio/eac3'), 'a box with Dolby says so')

// --- Seek previews ----------------------------------------------------------------
// The order frames get decoded in, and the stand-in shown while one still is.
// Both are arithmetic the player can't tell you it got wrong: it just quietly
// stops previewing.
const BUCKET = 5
const COARSE = BUCKET * 128

for (const duration of [7200, 1320, 300, 47, 5, 0]) {
  const order = [...walkOrder(duration, BUCKET, COARSE)]
  const seen = new Set(order)
  const want = Math.ceil(duration / BUCKET)

  assert.equal(seen.size, want, `${duration}s: every bucket reached`)
  for (let at = 0; at < duration; at += BUCKET)
    assert.ok(seen.has(at), `${duration}s: ${at} reached`)

  // The hover rounds down to a multiple of BUCKET. Land anywhere else and the
  // walk fills a cache the hover never reads.
  for (const at of seen)
    assert.equal(at, Math.floor(at / BUCKET) * BUCKET, `${duration}s: ${at} is on the grid`)

  // Re-walking earlier passes is what buys the subdivision; 8 passes over a
  // film is the ceiling, and it must not creep past that.
  assert.ok(order.length <= seen.size * 2 + 8, `${duration}s: ${order.length} steps for ${seen.size} frames`)
}

// Coarse first is the whole point: the opening pass has to span the film, not
// camp at the start, or the far half previews nothing until the walk gets there.
const twoHours = [...walkOrder(7200, BUCKET, COARSE)]
assert.ok(twoHours.slice(0, 12).some(at => at > 7200 * 0.8), 'the first pass reaches the end')
assert.equal(twoHours[1], COARSE, 'and it steps by the coarse stride, not the fine one')

const frames = new Map([[0, 'a.jpg'], [600, 'b.jpg'], [900, ''], [1200, 'c.jpg']])
assert.equal(nearestFrame(frames, 600, 60), 'b.jpg', 'its own frame wins')
assert.equal(nearestFrame(frames, 630, 60), 'b.jpg', 'a near one stands in')
assert.equal(nearestFrame(frames, 780, 60), null, 'nothing near enough to be worth showing')
// A position ffmpeg got nothing at is cached as '' — it is not a frame, and must
// never be handed back as one.
assert.equal(nearestFrame(frames, 900, 60), null, 'an empty is not a stand-in')
assert.equal(nearestFrame(frames, 890, 60), null)
assert.equal(nearestFrame(new Map(), 0, 60), null, 'an empty cache answers nothing')

// The bar's tooltips are the one overlay this repo doesn't own the markup of:
// Vuetify teleports them out of the player, so MpvPlayer's cutout selector has
// to name Vuetify's own classes to get them punched out of mpv's window. A
// rename upstream would break that silently, and only on X11 and Win32 — the
// two targets a browser check can't see.
const tooltipCss = await Bun.file('node_modules/vuetify/lib/components/VTooltip/VTooltip.sass').text()
assert.match(tooltipCss, /\.v-tooltip\n\s+> \.v-overlay__content/, 'vuetify still nests tooltip content the way the tracker looks for it')
const mpv = await Bun.file('app/components/MpvPlayer.vue').text()
assert.match(mpv, /const CUT = '\[data-cut\], \.v-tooltip > \.v-overlay__content'/, 'and the tracker still looks for it')
assert.match(mpv, /cutouts: overlay \? cutouts/, 'HUD holes must punch even before the first frame — Win32 hide does not always drop hit testing')
assert.doesNotMatch(mpv, /cutouts: visible && overlay/, 'empty cutouts while opening left Back/Retry dead under mpv')
assert.ok(!mpv.includes('rootEl.value?.querySelectorAll'), 'scoped to the player, a teleported tooltip is never found')
assert.match(mpv, /live: isLive\.value/, 'mpv must know live from VOD so 4K HEVC gets a real probe')
assert.match(mpv, /isLive\.value \? 'sdr'/, 'live HDR passthrough on SDR is a black picture with sound')

// --- HLS: the one thing a live channel needs and a torrent stream never does ---
// Chromium hands `<video>` an `.m3u8` and reports a corrupt file, so every live
// playlist has to go through hls.js there — and must NOT on Safari, which plays
// it natively and buffers it better than any JS demuxer can. The branch is the
// whole of it, so both directions are pinned.
{
  const safari = fakeVideo() as ReturnType<typeof fakeVideo> & { canPlayType: (t: string) => string }
  safari.canPlayType = t => (t.toLowerCase().includes('mpegurl') ? 'maybe' : '')
  const engine = videoEngine(safari as any)
  await engine.start('http://127.0.0.1:3032/premium-stream/tok.m3u8')
  assert.equal(
    safari.src,
    'http://127.0.0.1:3032/premium-stream/tok.m3u8',
    'native HLS goes straight to the element - no hls.js in front of it',
  )

  const chromium = fakeVideo() as ReturnType<typeof fakeVideo> & { canPlayType: (t: string) => string }
  chromium.canPlayType = () => ''
  const engine2 = videoEngine(chromium as any)
  // Under bun there is no MediaSource, so `Hls.isSupported()` is false and the
  // attach declines. What matters is that declining still plays: the element
  // gets the URL rather than the engine giving up.
  await engine2.start('http://127.0.0.1:3032/premium-stream/tok.m3u8')
  assert.equal(chromium.src, 'http://127.0.0.1:3032/premium-stream/tok.m3u8', 'a failed attach still hands the element the URL')
  assert.equal(engine2.status().running, true)

  // And a progressive file is never routed through the playlist path, whatever
  // the webview says about HLS.
  const plain = fakeVideo() as ReturnType<typeof fakeVideo> & { canPlayType: (t: string) => string }
  plain.canPlayType = () => ''
  const engine3 = videoEngine(plain as any)
  await engine3.start('http://127.0.0.1:3030/torrents/1/stream/0')
  assert.equal(plain.src, 'http://127.0.0.1:3030/torrents/1/stream/0')
}

const mpvLinux = readFileSync(new URL('../src-tauri/src/player.rs', import.meta.url), 'utf8')
assert.match(mpvLinux, /player_direct::cache_cli/, 'Direct HTTP cache flags live in one place')
assert.match(mpvLinux, /player_direct::play_url/, 'Direct HTTP opens through the loopback proxy, not lavf hopping 302s')
assert.doesNotMatch(mpvLinux, /resolved_url/, 'must not Range-GET a debrid URL before mpv — that spends the token')
const mpvWin = readFileSync(new URL('../src-tauri/src/player_windows.rs', import.meta.url), 'utf8')
assert.match(mpvWin, /player_direct::play_url/, 'Windows Direct path uses the same proxy wrap')
assert.match(mpvWin, /embed.set_shape\(width, height, &cutouts\)/, 'Win32 must punch HUD holes even when the surface is flagged hidden')
assert.match(mpvLinux, /embed.set_shape\(width, height, &cutouts\)/, 'X11 must punch HUD holes even when the surface is flagged hidden')
const mpvMac = readFileSync(new URL('../src-tauri/src/player_macos.rs', import.meta.url), 'utf8')
assert.match(mpvMac, /player_direct::play_url/, 'macOS Direct path uses the same proxy wrap')
const mpvDirect = readFileSync(new URL('../src-tauri/src/player_direct.rs', import.meta.url), 'utf8')
assert.match(mpvDirect, /cache-secs=1/, 'Direct HTTP must not wait on a 20s mpv cache')
assert.match(mpvDirect, /probesize=524288/, 'Direct MKV needs a real lavf probe, not 64KiB')
assert.match(mpvDirect, /fflags=\+nobuffer/, 'Direct HTTP must not sit in lavf readahead before the first frame')
assert.match(mpvDirect, /fn live_cli/, '4K live probe/cache is not the Direct 512KiB nobuffer path')
assert.match(mpvDirect, /probesize=8388608/, 'a 4K HEVC IDR does not fit in 512KiB')
assert.match(mpvDirect, /fflags=\+genpts/, 'live must replace Direct nobuffer or the picture is the frames that got dropped')
assert.match(mpvDirect, /hls-bitrate=max/, 'HLS live must pick the 4K rung, not the first 720p one')
assert.match(mpvDirect, /fn picture_cli/, '4K into a 1080p window needs a real scaler, not hermite')
assert.match(mpvDirect, /dscale=lanczos/, 'mitchell + linear-downscaling is why UHD still looked like a 720p upscale')
assert.doesNotMatch(mpvDirect, /linear-downscaling=yes/, 'linear light downscale is the soft-UHD look')
assert.match(mpvDirect, /skiploopfilter=none/, 'CPU skip-loop-filter is the other soft-4K path')
assert.match(mpvLinux, /player_direct::picture_cli/, 'Linux uses the shared picture flags')
assert.match(mpvLinux, /--vo=gpu/, 'X11 nearest-neighbour is why UHD looked soft after a correct decode')
assert.match(mpvWin, /player_direct::picture_cli/, 'Windows uses the shared picture flags')
assert.match(mpvMac, /player_direct::picture_kv/, 'macOS uses the shared picture flags')
assert.match(mpvLinux, /player_direct::live_cli/, 'Linux live uses the live flags')
assert.doesNotMatch(mpvLinux, /--hwdec=no/, 'software 4K HEVC skip-filters is why live still looked like 720p once vo=gpu can show 10-bit')
assert.match(mpvLinux, /--hwdec=auto-safe/, 'copy-back + gpu vo is the 10-bit picture, not a black window')
assert.match(mpv, /rec\.dw === 'number'/, 'the resolution badge must use display size, not coded 1920 on a 4K stream')
assert.match(mpv, /liveStallSince/, 'live buffering forever must reopen the URL, not wait on cache-pause')
assert.match(mpvWin, /player_direct::live_cli/, 'Windows live uses the same probe/cache')
// Live 4K is PQ/HLG into an SDR swapchain. The IPC pass covers it once mpv is
// up; these cover the frames before that, and both desktop backends need them
// or the two files have quietly drifted again.
for (const [name, src] of [['Linux', mpvLinux], ['Windows', mpvWin]] as const) {
  assert.match(src, /--target-trc=bt\.1886/, `${name} live opens SDR, not a washed-out PQ frame`)
  assert.match(src, /--target-colorspace-hint=no/, `${name} live does not claim an HDR window it hasn't got`)
}
assert.match(mpvMac, /player_direct::live_kv/, 'macOS live uses the same probe/cache')
assert.match(mpvDirect, /fn live_stream_lavf_o/, 'live must not inherit Direct\'s 90s rw_timeout')
assert.match(mpvDirect, /reconnect_at_eof=1/, 'a silent live panel closes without FIN — reopen, do not sit on Buffering')
assert.match(mpvLinux, /live_stream_lavf_o/, 'Linux live uses the live lavf reconnect flags')
assert.match(mpvWin, /live_stream_lavf_o/, 'Windows live uses the live lavf reconnect flags')
assert.match(mpvMac, /live_stream_lavf_o/, 'macOS live uses the live lavf reconnect flags')
assert.match(mpvDirect, /cache-pause-wait=3/, 'live must resume with a cache cushion, not 0.2s from Direct')
assert.match(mpvDirect, /proxy_free_stream_url/, 'wrap is the existing IPTV proxy, not a second GET')
const libSrc = readFileSync(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8')
assert.match(libSrc, /async fn thumbnail[\s\S]*player_direct::play_url/, 'seek previews take the same proxy wrap or Direct hover waits 40s')
const sliderSrc = readFileSync(new URL('../app/components/PlayerSlider.vue', import.meta.url), 'utf8')
assert.match(sliderSrc, /aspect-video/, 'hover card reserves a scene box before the JPEG lands')
assert.match(sliderSrc, /@focus="onFocus"/, 'a remote on the seek bar must preview, not only a pointer')
assert.match(sliderSrc, /^\s+data-cut$/m, 'the scene card is always punched out of the native window')
const watchSrc = readFileSync(new URL('../app/pages/watch.vue', import.meta.url), 'utf8')
assert.doesNotMatch(watchSrc, /player_warm/, 'warming a Direct URL before mpv spends the link')
const htmlSrc = readFileSync(new URL('../app/utils/htmlvideo.ts', import.meta.url), 'utf8')
assert.match(htmlSrc, /maxBufferLength: live \? 12 : 4/, 'hls.js Direct play must not prefetch 30s before starting')
assert.match(htmlSrc, /127\.0\.0\.1:3031\/stream/, 'Android / <video> Direct path uses the same proxy wrap')
assert.match(htmlSrc, /hasVlcPlayer\(\)/, 'Android wraps even if isTauri() is late — libVLC on the raw resolver is the 40s wait')
assert.doesNotMatch(htmlSrc, /invoke<string>\('proxy_free_stream_url'/, 'the wrap is a string, not an IPC round-trip before start')
const proxySrc = readFileSync(new URL('../src-tauri/src/iptv/proxy.rs', import.meta.url), 'utf8')
assert.match(proxySrc, /fn stream_http/, 'stream proxy must not inherit the 300s IPTV JSON timeout')
assert.match(proxySrc, /connect_timeout\(Duration::from_secs\(15\)\)/, 'dead hosts fail the connect, not a body deadline')
assert.match(proxySrc, /\.gzip\(false\)/, 'gzip on a movie strips Content-Length and Direct play buffers as chunked')
assert.match(proxySrc, /is_head/, 'lavf HEAD must not become a full GET of the file')
assert.match(proxySrc, /cached_redirect/, 'mpv re-probes must hit the cached CDN URL, not the resolver again')
assert.match(proxySrc, /fn prefer_highest_hls_rung/, 'HLS master must put the 4K rung first, not the phone 720p default')
assert.match(proxySrc, /strip_prefix\("#EXT-X-STREAM-INF:"\)/, 'BANDWIDTH sits on the same token as the tag — a naive split never sees it')
assert.match(proxySrc, /IPTV_PLAYER_UA/, 'Xtream must see a player UA; Chrome is the 720p HTML5 transcode')
assert.match(proxySrc, /VLC\/3\.0\.18/, 'same player UA streaming_m3u already uses')
assert.match(proxySrc, /LIVE_STALL/, 'a silent live TCP must be cut so lavf can reconnect')
assert.match(proxySrc, /HTTP\/1\.0/, 'chunked live TS is why lavf never reconnects')
assert.match(proxySrc, /fn is_live_mpegts/, 'Range on live MPEG-TS steals the one Xtream slot')
assert.match(proxySrc, /fn write_live_head/, 'lavf HEAD must not open a live stream slot')
const vlcKt = readFileSync(new URL('../src-tauri/gen/android/app/src/main/java/io/github/rivulet/rivulet/VlcPlayer.kt', import.meta.url), 'utf8')
assert.doesNotMatch(vlcKt, /skiploopfilter=4/, 'skipping every HEVC loop filter is why Android 4K looked blocky')
assert.doesNotMatch(vlcKt, /skipidct=4/, 'skipping IDCT is the same soft-UHD path')
assert.match(vlcKt, /avcodec-skiploopfilter=0/, 'libVLC must keep the HEVC loop filter on 4K')
// Halved once the proxy started capping this device's ladder at 1080: three
// seconds of buffer is three seconds before the first frame, which on live TV
// is the difference between a channel that starts and one that "keeps loading".
assert.match(vlcKt, /live-caching=1500/, 'live starts on a buffer sized for what this device is actually sent')
assert.match(vlcKt, /network-caching=1500/, 'and the network buffer matches it')

// --- Android full screen: after playback starts, never during it ---------------
// The watch page keys the player on `src`, so one start mounts it twice. Entering
// full screen on mount (and leaving on unmount) rotated the phone landscape,
// portrait, landscape while libVLC stopped and restarted under it — and neither
// torrents nor direct links played at all, where v0.6.14 without it did.
const mountBlock = /\nonMounted\(\(\) => \{[\s\S]*?\n\}\)/.exec(mpv)?.[0] ?? ''
assert.ok(mountBlock.length > 0, 'the player\'s onMounted is where it was')
assert.doesNotMatch(mountBlock, /isAndroid\(\)/, 'Android must not enter full screen on mount: it rotates the phone twice per start')
const autoBlock = /let autoFullscreen = false[\s\S]*?\n\)/.exec(mpv)?.[0] ?? ''
assert.match(autoBlock, /position\.value > 0\.5/, 'Android full screen waits for playback time to move')
assert.match(autoBlock, /!buffering\.value/, 'and for buffering to have stopped')
assert.match(autoBlock, /setWindowFullscreen\(true\)/, 'and then goes full screen')
// Running at background priority, the decoder scan held Android's codec-list
// lock while starved — and libVLC's own decoder setup waits on that lock.
assert.doesNotMatch(vlcKt, /THREAD_PRIORITY_BACKGROUND/, 'the decoder scan must not be starved while holding the codec lock')

// --- Live on Android: a moving clock is a picture ------------------------------
// libVLC often reports no size for a live channel, so videoWidth === 0 alone kept
// the loader up over a playing picture, let the 12-second start watchdog declare a
// playing channel dead, and never let a real start reset the reconnect counter.
assert.match(mpv, /const moving = ref\(false\)/, 'the player tracks whether the clock is moving')
assert.match(mpv, /videoWidth\.value > 0 \|\| moving\.value \|\| duration\.value/, 'the start watchdog must not kill a channel whose clock is moving')
assert.match(mpv, /videoWidth\.value === 0 && !moving\.value\)[\t\v\f\r \xA0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000\uFEFF]*\n\s*return 'loading'/, 'the loader clears once the clock moves, size or no size')
assert.match(mpv, /defineExpose\(\{[\s\S]*?\bmoving,/, 'the watch pages can read it')
const premiumWatch = readFileSync(new URL('../app/pages/live-tv/premium/watch.vue', import.meta.url), 'utf8')
const freeWatch = readFileSync(new URL('../app/pages/live-tv/watch.vue', import.meta.url), 'utf8')
assert.match(premiumWatch, /p\.videoWidth > 0\) \|\| asBool\(p\.moving\)/, 'Premium TV counts a moving clock as a picture, so a real start resets the reconnect counter')
assert.match(freeWatch, /p\.videoWidth > 0\) \|\| asBool\(p\.moving\)/, 'Free TV counts it too')

// …and some live HLS on Android plays with neither a size nor a moving clock, so
// both pages sat on "Connecting" over a channel that was plainly on screen.
// libVLC's Vout event is the one signal it always sends; mpv calls it vo-configured.
assert.match(vlcKt, /if \(event\.type == MediaPlayer\.Event\.Vout\) \{[\s\S]*?voutCount = event\.voutCount/, 'Android counts its video outputs')
assert.match(vlcKt, /\.put\("vo-configured", voutCount > 0\)/, 'and reports them as vo-configured')
assert.match(vlcKt, /cacheFill = 0\s+voutCount = 0/, 'a new start forgets the last channel\'s output')
assert.match(mpv, /POLLED = \[[^\]]*'vo-configured'/, 'the player polls it')
assert.match(mpv, /\|\| p\['vo-configured'\] === true\)/, 'and a video output that is up counts as a picture')
assert.match(htmlSrc, /'vo-configured': \(\) => video\.videoWidth > 0/, 'the <video> shim answers it too')

// --- Why a channel never opened -----------------------------------------------
// A stream that never arrives fires no libVLC event at all: `EncounteredError`
// is a decoder failure, so "Connecting…" sat there with nothing anywhere to
// explain it. The event trace is that account, and it has to reach the screen —
// a phone has no logcat.
assert.match(vlcKt, /private val trace = java\.util\.concurrent\.ConcurrentLinkedDeque/, 'Android keeps its last libVLC events')
assert.match(vlcKt, /while \(trace\.size > 24\) trace\.pollFirst\(\)/, 'and the trace is capped')
assert.match(vlcKt, /fun safeUrl\(url: String\): String = url\.substringBefore\('\?'\)/, 'the URL is recorded without its query')
assert.match(vlcKt, /note\("start \$\{safeUrl\(url\)\}"\)/, 'so no provider credential is ever traced')
assert.match(vlcKt, /android\.util\.Log\.d\("RivuletPlayer"/, 'and every line goes to logcat, which is where a trace belongs')
assert.doesNotMatch(vlcKt, /\.put\("trace"/, 'status does not carry it: nothing reads it, and it was two dozen strings every poll')
// Anything still open holds the provider's slot, and on a one-connection
// account that is the next channel refused.
assert.match(vlcKt, /if \(hasMedia\) p\.stop\(\)/, 'a new stream closes the old one before it opens')
assert.match(vlcKt, /MediaPlayer\.Event\.Opening -> note\("Opening"\)/, 'opening is traced, which is where a stuck channel stops')
// ...but the trace is a logcat aid, not something to paint over the picture.
// It shipped on screen in 0.6.28 to catch the bug below, and came straight
// back off once it had.
const overlaySrc = readFileSync(new URL('../app/components/live-tv/LivePlayerOverlay.vue', import.meta.url), 'utf8')
assert.doesNotMatch(overlaySrc, /connectTrace/, 'the HUD does not draw the event log')
assert.doesNotMatch(freeWatch, /connect-trace/, 'nor does Free TV hand it one')
assert.doesNotMatch(premiumWatch, /connect-trace/, 'nor does Premium')

// --- A live stream that hiccups is not a dead one -----------------------------
// `running` was one-way: set in `start`, cleared by the first EndReached, never
// set again. libVLC raises that on an HLS discontinuity and reconnects itself,
// so one hiccup marked a playing channel stopped for the rest of its life — the
// page stopped polling, the HUD stuck on "Connecting" over a live picture, and
// the auto-skip walked off down the list. Only Retry, a fresh `start`, cleared
// it, which is exactly why Retry "worked immediately" every time.
// Exact lines, no wildcard between them: a `[\s\S]*?running = true` also matches
// the words inside a comment, so commenting the assignment out slipped past it.
assert.match(
  vlcKt,
  /\n {10}running = true\r?\n {10}deadAt = 0L\r?\n {10}failure = null/,
  'a picture puts the player back to running — that `running` was one-way is the whole bug',
)
assert.match(vlcKt, /markDead\("libVLC could not open this stream\."\)/, 'an error takes the grace path')
assert.match(vlcKt, /markDead\(null\)/, 'and so does EndReached')
assert.match(
  vlcKt,
  /fun isLiveStream\(\): Boolean = \(player\?\.length \?: 0L\) <= 0L/,
  'live is "reports no length", which is what keeps a file ending the instant it says so',
)
assert.match(
  vlcKt,
  /if \(dead != 0L && android\.os\.SystemClock\.elapsedRealtime\(\) - dead > liveRecoverMs\)/,
  'only the grace window may declare a live stream dead',
)
assert.match(
  vlcKt,
  /if \(isLiveStream\(\) && voutCount > 0\) \{/,
  'but only a stream that showed a picture earns the grace — a channel that never opened must still fail fast, or Free TV crawls down the list',
)
assert.match(
  mpv,
  /if \(isLive\.value && !confirmedStopped && moving\.value\) \{/,
  'and the page takes a second reading before tearing down a live stream that was playing',
)
assert.match(mpv, /if \(!isLive\.value && position\.value > 0 &&/, 'live has no end to reach, so it is never "ended"')

// --- The provider's one slot is usually held by us ----------------------------
// A pre-flight `probeAccount` refused to start whenever activeConnections was
// at the limit — which on a one-slot account is true while our own previous
// channel is still open, or while the panel has yet to release it. So it
// refused streams that would have played, and spent a round trip per start
// doing it. The probe belongs after a real 401/403, and nowhere else.
assert.match(premiumWatch, /No pre-flight connection probe/, 'the reason it is gone is written down')
assert.equal(
  (premiumWatch.match(/premium\.probeAccount\(\)/g) ?? []).length,
  2,
  'probeAccount runs only on the two refusal paths, never before a start',
)

// --- Nothing on loopback may hang forever -------------------------------------
// `fetch` has no timeout of its own, the API shares this process, and a cold
// start on Android can have the page mounted before the listener is bound. With
// nothing ever rejecting, `loadStatus` never left 'loading' and the Premium TV
// page spun for good — the error branch that would have said so was unreachable.
const premiumApiSrc = readFileSync(new URL('../app/utils/premiumTv.ts', import.meta.url), 'utf8')
assert.match(premiumApiSrc, /const REQUEST_TIMEOUT_MS = 20_000/, 'every premium API call has a ceiling')
assert.match(premiumApiSrc, /ctrl\.abort\(timedOut\(\)\)/, 'and one that runs past it is aborted')
assert.match(premiumApiSrc, /'TimeoutError'/, 'under a name of its own: a caller\'s cancel is swallowed by design, a timeout must not be')
assert.match(premiumApiSrc, /signal: ctrl\.signal/, 'the fetch obeys our controller, not only the caller\'s')
const premiumStore = readFileSync(new URL('../app/stores/premiumTv.ts', import.meta.url), 'utf8')
assert.match(premiumStore, /for \(let attempt = 0; attempt < 2; attempt\+\+\)/, 'and status is asked twice before it is called an error')
// The fetch timeout was not enough, because the hang was before the fetch.
// `premium_api_token` is a synchronous Tauri command that reads the OS
// keychain, and `mintToken` caches its in-flight promise — so one wedged
// invoke wedged every request after it for the life of the process, and the
// settings page sat on disabled spinners with no error anywhere.
assert.match(premiumApiSrc, /async function bounded<T>\(work: Promise<T>, ms: number\)/, 'IPC is bounded too')
assert.match(premiumApiSrc, /bounded\(\s*invoke<\{ token: string, expiresAt: number \}>\('premium_api_token'\)/, 'minting a token cannot hang for ever')
assert.match(premiumApiSrc, /bounded\(\s*invoke\('premium_set_entitlement'/, 'nor can pushing the entitlement')

// --- The installer cannot overwrite a running mpv -----------------------------
// mpv is its own process and outlives an app that crashed, so an upgrade failed
// on "Error opening file for writing: ...mpv.exe" with Abort/Retry/Ignore.
// Tauri's template closes the app; it knows nothing about the app's children.
const hooks = readFileSync(new URL('../src-tauri/installer-hooks.nsh', import.meta.url), 'utf8')
assert.match(hooks, /!macro NSIS_HOOK_PREINSTALL/, 'the installer clears leftovers before it writes')
assert.match(hooks, /Where-Object Path -like '\*Rivulet\*'/, 'and only ours — a stranger\'s mpv on the PATH is none of our business')
// Killing the player was not enough: the installer log showed the kill running
// and the next extract still refusing, because the app was alive to start
// another one. Windows will not overwrite a running binary but will rename one,
// so the name is taken away from whatever holds it and the stale copy is queued
// for the next reboot. That path does not depend on winning a race.
assert.match(hooks, /Rename "\$\{FILE\}" "\$\{FILE\}\.old"/, 'a locked binary is moved aside rather than fought over')
assert.match(hooks, /Delete \/REBOOTOK "\$\{FILE\}\.old"/, 'and the stale copy is not left as clutter')
assert.match(hooks, /!insertmacro RIVULET_FREE_FILE .*mpv\.exe/, 'mpv, the file that actually failed, is the one it frees')
assert.match(hooks, /-Name rivulet,mpv/, 'and the app goes first, so nothing starts a new player behind us')
const tauriConf = readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8')
assert.match(tauriConf, /"installerHooks": "\.\/installer-hooks\.nsh"/, 'and the bundler is actually told to run them')

// --- The first click on a channel ---------------------------------------------
// `ensure` builds the TextureView, and its SurfaceTexture does not exist until a
// layout pass later — so the first start called `play()` with nowhere to draw.
// libVLC decoded into nothing: no vout, no picture, "Connecting" for good, while
// Retry played at once because by then the surface was there. That the report
// was always "the first click" is not a coincidence, it is the construction.
assert.match(vlcKt, /if \(outputAttached\) \{\r?\n {8}p\.play\(\)/, 'playback waits until there is somewhere to draw')
assert.match(vlcKt, /main\.postDelayed\(playWhenReady, surfaceWaitMs\)/, 'bounded, so a surface that never arrives still gets sound')
assert.match(vlcKt, /if \(pendingPlay\) \{/, 'and the surface arriving is what starts it')

// eslint-disable-next-line no-console
console.log('player: ok')
