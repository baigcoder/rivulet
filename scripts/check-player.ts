import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { deviceCodecs, hasNativePlayer, hasVideoOverlay, videoEngine, vlcEngine } from '../app/utils/htmlvideo'
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
const mpvMac = readFileSync(new URL('../src-tauri/src/player_macos.rs', import.meta.url), 'utf8')
assert.match(mpvMac, /player_direct::play_url/, 'macOS Direct path uses the same proxy wrap')
// keep-open holds the process through the first empty read. cache-pause-initial
// would then wait for a second of timeline — a whole HEVC GOP, the "starts at
// 1%" stall — so the picture begins as soon as a frame decodes.
assert.match(mpvLinux, /--cache-pause-initial=no/, 'Linux starts the torrent picture on the first frame')
assert.match(mpvLinux, /if engine \{[\s\S]*--keep-open=yes/, 'Linux torrent mpv must not exit on the first empty read')
assert.match(mpvWin, /if engine \{[\s\S]*--keep-open=yes/, 'Windows torrent mpv must not exit on the first empty read')
assert.match(mpvMac, /keep-open", if engine_stream \{ "yes"/, 'macOS torrent mpv must not exit on the first empty read')
assert.match(mpvLinux, /reuse_engine_process/, 'Linux player_start is idempotent for a live engine stream')
assert.match(mpvWin, /reuse_engine_process/, 'Windows player_start is idempotent for a live engine stream')
assert.match(mpvMac, /reuse_engine_process/, 'macOS player_start is idempotent for a live engine stream')
assert.match(mpvWin, /--cache-pause-initial=no/, 'Windows starts the torrent picture on the first frame')
assert.match(mpvMac, /set\("cache-pause-initial", "no"\)/, 'macOS starts the torrent picture on the first frame')
const vlcAndroid = readFileSync(new URL('../src-tauri/gen/android/app/src/main/java/io/github/rivulet/rivulet/VlcPlayer.kt', import.meta.url), 'utf8')
assert.match(vlcAndroid, /val cacheMs = if \(torrentStream\) 1500 else 300/, 'Android gives growing torrent streams a startup cache')
const mpvDirect = readFileSync(new URL('../src-tauri/src/player_direct.rs', import.meta.url), 'utf8')
assert.match(mpvDirect, /fn reuse_engine_process/, 'a second player_start of the same engine URL must not kill FileStream')
assert.match(mpvDirect, /cache-secs=20/, 'Direct HTTP has 20s mpv cache for smooth playback')
assert.match(mpvDirect, /probesize=1048576/, 'Direct MKV needs 1MB lavf probe')
assert.match(mpvDirect, /cache-pause=no/, 'Direct HTTP starts playing immediately')
assert.match(mpvDirect, /fn live_cli/, '4K live probe/cache is not the Direct 512KiB nobuffer path')
assert.match(mpvDirect, /probesize=8388608/, 'a 4K HEVC IDR does not fit in 512KiB')
assert.match(mpvDirect, /fflags=\+genpts/, 'live must replace Direct nobuffer or the picture is the frames that got dropped')
assert.match(mpvLinux, /player_direct::live_cli/, 'Linux live uses the live flags')
assert.match(mpvLinux, /--hwdec=no/, 'X11 --wid + HEVC 10-bit hwdec paints black with sound')
assert.match(mpvLinux, /if engine \|\| local_file \{[\s\S]*--hwdec=no/, 'Linux torrent HEVC uses reliable software frames in the embedded X11 player')
assert.match(mpvWin, /player_direct::live_cli/, 'Windows live uses the same probe/cache')
assert.match(mpvMac, /player_direct::live_kv/, 'macOS live uses the same probe/cache')
assert.match(mpvDirect, /http_seekable=0/, 'ffmpeg HTTP must not Range the cue index either')
assert.match(mpvDirect, /force-seekable=no/, 'mpv must not treat a growing torrent file as seekable')
// An unseekable stream can only be dragged over what mpv is holding, and both
// caps are seconds-limits on the same unchanged `demuxer-max-bytes` budget.
assert.match(mpvDirect, /--cache-secs=600/, 'a growing torrent buffers minutes, not the 30s that made the seek bar look broken')
assert.match(mpvDirect, /--demuxer-readahead-secs=300/, 'and reads that far ahead, or the cap above never fills')
assert.match(mpvDirect, /\("cache-secs", "600"\)/, 'macOS keeps the same window')
assert.match(mpvDirect, /timeout=60000000/, 'live IPTV gets a 60s first-byte window, not Direct\'s 15s')
assert.match(mpvLinux, /stream_lavf_o\(engine, live/, 'Linux live vs torrent lavf flags are distinct')
assert.match(mpvWin, /stream_lavf_o\(engine, live/, 'Windows live vs torrent lavf flags are distinct')
assert.match(mpvMac, /stream_lavf_o\(\s*engine_stream,\s*live/, 'macOS live vs torrent lavf flags are distinct')
// Seeking a growing torrent is gated on its tail already being downloaded:
// mpv reads a matroska's cues from the end before it plays a frame, and asking
// for that too early stalls the open and sends the swarm to the wrong end.
assert.match(mpvDirect, /pub fn stream_lavf_o\(engine: bool, live: bool, seekable: bool\)/, 'seekability is a parameter, not a constant')
for (const [name, src] of [['Linux', mpvLinux], ['Windows', mpvWin], ['macOS', mpvMac]] as const)
  assert.match(src, /seekable: Option<bool>/, `${name} player_start takes the seekable flag`)
assert.match(mpvDirect, /network-timeout=90/, 'ffmpeg 30s first-byte abort is the 40s Direct wait')
assert.match(mpvDirect, /proxy_free_stream_url/, 'wrap is the existing IPTV proxy, not a second GET')
const libSrc = readFileSync(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8')
assert.match(libSrc, /async fn thumbnail[\s\S]*player_direct::play_url/, 'seek previews take the same proxy wrap or Direct hover waits 40s')
assert.match(libSrc, /fn apply_bundled_ytdlp/, 'mpv --ytdl must see the bundled yt-dlp, not the AppImage PATH')
assert.match(mpvLinux, /apply_bundled_ytdlp/, 'Linux embedded mpv uses bundled yt-dlp for YouTube URLs')
assert.match(mpvWin, /apply_bundled_ytdlp/, 'Windows embedded mpv uses bundled yt-dlp for YouTube URLs')
const sliderSrc = readFileSync(new URL('../app/components/PlayerSlider.vue', import.meta.url), 'utf8')
assert.match(sliderSrc, /aspect-video/, 'hover card reserves a scene box before the JPEG lands')
assert.match(sliderSrc, /@focus="onFocus"/, 'a remote on the seek bar must preview, not only a pointer')
assert.match(sliderSrc, /^\s+data-cut$/m, 'the scene card is always punched out of the native window')
const watchSrc = readFileSync(new URL('../app/pages/watch.vue', import.meta.url), 'utf8')
const detailView = readFileSync(new URL('../app/components/MediaDetailView.vue', import.meta.url), 'utf8')
assert.match(detailView, /onBeforeRouteLeave\(\(\) => stopHeroTrailer\(\)\)/, 'Play must wait for the cover trailer to release the decoder')
assert.match(detailView, /stopHeroTrailer\(\): Promise/, 'route leave must await the trailer unload, not fire-and-forget')
assert.match(detailView, /@pointerdown="stopHeroTrailer"/, 'Play starts unloading the trailer before the router navigates')
assert.doesNotMatch(watchSrc, /player_warm/, 'warming a Direct URL before mpv spends the link')
const playerSrc = readFileSync(new URL('../app/components/MpvPlayer.vue', import.meta.url), 'utf8')
assert.match(playerSrc, /torrentStartRetries < 3/, 'a torrent that exits before its first frame gets bounded retries')
// mpv exiting means its FileStream closed with it, so there is no download pin
// left for a reopen to disturb. Skipping the reopen because the swarm looked
// healthy stopped the poll and left the buffering spinner up for ever.
assert.doesNotMatch(playerSrc, /engineIsPulling/, 'a dead mpv must be reopened whatever the swarm is doing')
assert.match(playerSrc, /void startPlayer\(true\)/, 'an engine stream whose player exited is reopened rather than left spinning')
// Bound as `@click="startPlayer"`, the click event lands in `retryingTorrentStartup`
// and Retry silently keeps the exhausted retry budget instead of clearing it.
assert.match(playerSrc, /@click="\(\) => startPlayer\(\)"/, 'Retry must call startPlayer with no argument')
assert.match(playerSrc, /if \(prev\)\s*await stopPlayer/, 'the first engine src after an empty mount must start, not player_stop then start')
assert.match(playerSrc, /src === prev/, 'the same engine URL must not player_stop and reopen')
assert.match(playerSrc, /srcEpoch/, 'a Quality pick must not player_stop then bail on busy and leave a blank window')
assert.match(playerSrc, /resolving && !started\.value/, 'resolving a Quality pick must not punch out a picture that is already playing')
assert.match(playerSrc, /!props\.src && !started\.value && \(props\.step \|\| props\.status\)/, 'a missing src still shows Buffering, not a blank HUD')
assert.match(playerSrc, /engineFrameWatch/, 'a hung file:// copy is watched for a first frame')
assert.match(playerSrc, /if \(!fromDisk\.value\)\s*return/, 'engine HTTP must not restart mpv when the first frame is late')
assert.match(playerSrc, /armEngineFrameWatch/, 'the first-frame watch is armed once, not inlined at 2.5s')
assert.match(playerSrc, /seekable: false/, 'growing torrents open unseekable so the first GET is the FileStream, not a piece-map wait')
// The auto-English one-shot used to be spent by the refresh that runs the
// instant mpv opens, when the track list is still empty — so the later call,
// the one that has the tracks, never looked.
assert.match(playerSrc, /autoEng\.value && audioTracks\.value\.length/, 'English audio is chosen once there are tracks to choose between')
assert.match(playerSrc, /function englishAudio/, 'and eng/en plus commentary are its problem, not the caller\'s')
assert.doesNotMatch(playerSrc, /lang\?\.startsWith\('eng'\)/, 'a container tagged `en` is English too')
assert.doesNotMatch(playerSrc, /void restart\(true\)/, 'reopening engine HTTP drops the FileStream and loops Buffering 0%')
assert.doesNotMatch(playerSrc, /\}, 2500\)/, 'restarting at 2.5s drops the FileStream and loops 0:00')
assert.doesNotMatch(playerSrc, /Range: 'bytes=0-131071'/, 'a Range header on the engine probe is a CORS preflight the pin used to skip')
assert.doesNotMatch(playerSrc, /enginePin|prefetchEngineTail|releaseEngineHold/, 'a held GET plus an EOF tail is two FileStreams fighting mpv for the wrong end of the file')
assert.doesNotMatch(playerSrc, /NEED = 131072/, 'an engine header probe that keeps a GET open starves mpv of the first pieces')
assert.doesNotMatch(playerSrc, /waitForStream\(props\.src/, 'a GET of the engine stream before mpv is a 32MB FileStream that starves the first pieces')
assert.match(playerSrc, /qualityChoices\.length/, 'available torrent qualities show on the buffering overlay')
assert.match(playerSrc, /fromTorrent \|\| activeQuality/, 'torrent Play (engine or disk) always offers the Quality control')
assert.match(playerSrc, /seenFrame/, 'the stalled overlay stays up until a real frame, not an interpolated clock')
assert.match(playerSrc, /fromTorrent\.value && started\.value && !seenFrame\.value/, 'a torrent at 0:00 keeps Buffering on top of --wid')
assert.match(playerSrc, /visible = !!props\.src && started\.value/, 'an empty src must not remap leftover mpv over the spinner')
assert.match(playerSrc, /!fromTorrent\.value && !paused\.value/, 'the rAF clock must not invent time-pos on a torrent still at 0:00')
assert.match(playerSrc, /fromEngine\.value && p\['time-pos'\] > 0\.5/, 'real playback clears the torrent startup retry count')
assert.match(playerSrc, /emit\('diskStuck'\)/, 'a finished file:// copy stuck at 0:00 falls back to the engine stream')
assert.doesNotMatch(playerSrc, /Punch the whole box out of the native window/, 'unmapping/full-cutting --wid before the first frame deadlocks file:// at 0:00')
assert.match(watchSrc, /preferStream:\s*!readySrc/, 'title Play opens the engine HTTP stream, not a first-spawn file://')
// The engine stream is unseekable on purpose (`force-seekable=no`), so a copy
// that is fully on disk has to be opened as a path or it plays with no
// duration and a seek bar that does nothing.
assert.match(watchSrc, /const nextSrc = wantDisk \? disk : \(readySrc\.startsWith\(ENGINE\) \? readySrc : http\)/, 'a finished copy opens as a path; a growing one keeps the engine stream')
assert.match(watchSrc, /wantDisk = namedDisk \|\| !!listed\?\.stats\?\.finished/, 'and waits for the file list, which heldSrc needs to name that path')
const downloadsPage = readFileSync(new URL('../app/pages/downloads.vue', import.meta.url), 'utf8')
assert.match(downloadsPage, /downloads\.titleFor\(t\.info_hash\)/, 'Downloads Play hands the player the title it was filed under, or the pause overlay has a filename and no artwork')
assert.match(downloadsPage, /torrentAction\(t\.id, 'start'\)/, 'Downloads Play unpauses the torrent before it opens the stream')
assert.match(downloadsPage, /pickVideoFile\(t\.files \?\? \[\]/, 'and navigates on the files already on the row, not a details round trip')
assert.match(downloadsPage, /settings\.downloadDir/, 'Open folder falls back to the storage setting when the engine omitted output_folder')
assert.match(downloadsPage, /resolvedDir/, 'and to the engine default when that setting is empty too')
assert.match(downloadsPage, /containingFolder/, 'and opens a directory, never the video file')
assert.match(watchSrc, /if \(!torrent\.value\)\s*torrent\.value = late\[0\]/, 'Quality pills fill as soon as sources answer, not after metadata')
assert.match(watchSrc, /void downloads\.focus\(started\.id\)/, 'first Play opens the stream without waiting on the downloads poll')
assert.match(watchSrc, /await downloads\.focus/, 'the torrent must be live before the stream probe, or a paused copy never grows')
const torrentUtil = readFileSync(new URL('../app/utils/torrents.ts', import.meta.url), 'utf8')
assert.match(torrentUtil, /export function tailBuffered/, 'whether the cues are on disk is a piece-bitfield read, not a GET of the stream')
assert.match(torrentUtil, /Prefix only/, 'headBuffered stops at the first missing piece — later ones do not start the picture')
assert.match(torrentUtil, /onAlternativesLate\?\.\(alts\)/, 'startTorrent publishes qualities before addTorrent waits')
assert.match(torrentUtil, /alreadyListed/, 'a hash the engine is already fetching is not POSTed again')
assert.match(torrentUtil, /listedTorrent/, 'first Play opens as soon as the engine lists the hash, not after the file list')
assert.match(torrentUtil, /onQueued/, 'the player is told the hash so it can open the stream during metadata')
assert.match(watchSrc, /openListedStream/, 'title Play polls the engine list and opens mpv before addTorrent returns')
// Switching source in torrent mode is an add like any other: it gets the same
// list-handoff so it starts on the first pieces, and the copy it walked away
// from is deleted rather than left part-downloaded on the disk.
assert.match(watchSrc, /onQueued: \(\{ hash, index \}\) => \{\s*if \(mine === generation\)/, 'a source switch opens as soon as the engine lists the hash')
assert.match(watchSrc, /function dropSwitchedAway/, 'and the copy left behind is cleaned up')
assert.match(watchSrc, /held\.stats\?\.finished\)[\t\v\f\r \xA0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000\uFEFF]*\n\s*return/, 'but never a finished one — that is a whole film someone can watch offline')
assert.match(watchSrc, /pickVideoFile/, 'a multi-file pack must open the video index, not stream/0 then switch')
assert.match(watchSrc, /if \(src\.value && !src\.value\.startsWith\(ENGINE\)\)/, 'a search must not player_stop an engine stream already opening')
assert.doesNotMatch(watchSrc, /waitForEngineHead/, 'a GET of the engine stream before mpv is a second FileStream')
assert.doesNotMatch(torrentUtil, /waitForEngineHead/, 'and it does not live in torrents either')
assert.match(watchSrc, /started\.id >= 0 \? started\.id : null/, 'disk playback keeps the engine id for stats and focus')
assert.match(watchSrc, /playUrl\(started\)/, 'disk paths and HTTP streams share one play-url helper')
assert.match(watchSrc, /fromDownloads/, 'Downloads Play opens the engine stream without fetching metadata')
assert.match(watchSrc, /heldSrc/, 'a finished download opens the disk path so the seek bar works')
assert.match(watchSrc, /streamUrl\(engineNow\.id/, 'a title already in the engine streams immediately while it downloads')
assert.match(watchSrc, /torrentAction\(engineNow\.id, 'start'\)/, 'a paused download is started before mpv opens the stream')
assert.match(watchSrc, /loadEngineQualities/, 'torrent engine Play lists other magnets as qualities while the stream is up')
assert.match(watchSrc, /onDiskStuck/, 'a hung file:// Play falls back to the engine HTTP stream')
assert.match(watchSrc, /adopt:\s*false/, 'a Quality pick must add that magnet, not keep the torrent already on disk')
assert.match(watchSrc, /preferStream:\s*true/, 'a Quality pick opens the engine stream, not a stuck file:// path')
assert.match(watchSrc, /cached:\s*null/, 'a Quality pick must not reuse the title\'s previous hash')
assert.match(watchSrc, /pickMagnet/, 'a Quality row with a hash still starts that torrent when the magnet field is empty')
assert.doesNotMatch(watchSrc, /if \(!next \|\| index === activeCandidate/, 'tapping the highlighted quality must retry, not no-op')
assert.match(watchSrc, /listed\?\.name/, 'torrent quality is read from the engine name, not the TMDB title')
assert.doesNotMatch(watchSrc, /:key="src \|\| 'idle'"/, 'fetching qualities must not remount the player onto a blank 0:00')
assert.match(
  watchSrc,
  /for \(let i = activeCandidate\.value \+ 1/,
  'a dead Direct/torrent copy walks every remaining candidate, not only the next index',
)
assert.match(playerSrc, /hasLaterCandidate/, 'a last candidate must show Playback failed, not a silent failed emit')
assert.match(playerSrc, /8_000/, 'Direct open watch is short enough to failover, long enough for a CDN')
assert.match(playerSrc, /centre === 'error'[\s\S]*\$t\('Back'\)/, 'Playback failed offers Back, not only Retry')
assert.match(playerSrc, /fromTorrent \? \$t\('Next quality'\)/, 'torrent failure offers the next magnet, Direct the next server')
assert.doesNotMatch(playerSrc, /12_000/, 'the 12s Direct hang is gone')
assert.match(mpvDirect, /fn is_local_file/, 'a finished copy is a file path, not the growing HTTP stream')
assert.doesNotMatch(mpvLinux, /--globbing=no/, 'mpv 0.41 treats --globbing as a fatal unknown option and never opens the stream')
assert.match(mpvLinux, /mpv exited immediately/, 'a flag mpv rejects must surface instead of a black 0:00 player')
assert.match(mpvLinux, /engine \|\| local_file/, 'Linux 10-bit HEVC off disk uses the same software VO as the engine stream')
assert.doesNotMatch(mpvLinux, /demuxer-mkv-probe-start-time=no/, 'a finished copy must read MKV cues so duration and seeking work')
assert.doesNotMatch(mpvWin, /--globbing=no/, 'Windows mpv must not die on an unknown --globbing flag either')
assert.match(mpvWin, /engine \|\| local_file/, 'Windows torrent files share the software VO path')
assert.doesNotMatch(mpvWin, /demuxer-mkv-probe-start-time=no/, 'Windows finished copies must read MKV cues too')
assert.match(mpvLinux, /player_direct::file_cli/, 'a finished copy must not inherit Direct HTTP reconnect/timeouts')
assert.match(mpvWin, /player_direct::file_cli/, 'Windows finished copies use the same local-file flags')
assert.match(mpvMac, /player_direct::file_kv/, 'macOS finished copies skip HTTP lavf options')
assert.match(mpvLinux, /if !local_file/, 'Linux does not pass stream-lavf-o on file://')
assert.match(mpvWin, /if !local_file/, 'Windows does not pass stream-lavf-o on file://')
assert.match(playerSrc, /if \(isLive\.value\)\s*return ''/, 'live connecting/error UI is the overlay, not a second centre card')
const htmlSrc = readFileSync(new URL('../app/utils/htmlvideo.ts', import.meta.url), 'utf8')
assert.match(htmlSrc, /maxBufferLength: 4/, 'hls.js Direct play must not prefetch 30s before starting')
assert.match(htmlSrc, /127\.0\.0\.1:3031\/stream/, 'Android / <video> Direct path uses the same proxy wrap')
assert.match(htmlSrc, /hasVlcPlayer\(\)/, 'Android wraps even if isTauri() is late — libVLC on the raw resolver is the 40s wait')
assert.doesNotMatch(htmlSrc, /invoke<string>\('proxy_free_stream_url'/, 'the wrap is a string, not an IPC round-trip before start')
const proxySrc = readFileSync(new URL('../src-tauri/src/iptv/proxy.rs', import.meta.url), 'utf8')
assert.match(proxySrc, /fn stream_http/, 'stream proxy must not inherit the 300s IPTV JSON timeout')
assert.match(proxySrc, /connect_timeout\(Duration::from_secs\(\d+\)\)/, 'dead hosts fail the connect, not a body deadline')
assert.match(proxySrc, /\.gzip\(false\)/, 'gzip on a movie strips Content-Length and Direct play buffers as chunked')
assert.match(proxySrc, /is_head/, 'lavf HEAD must not become a full GET of the file')
assert.match(proxySrc, /no reachable streams in playlist/, 'a master of unresolvable hosts is 502, not Connecting forever')

// eslint-disable-next-line no-console
console.log('player: ok')
