package io.github.rivulet.rivulet

import android.graphics.SurfaceTexture
import android.media.MediaCodecList
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.view.Surface
import android.view.TextureView
import android.view.View
import android.view.ViewGroup
import android.webkit.JavascriptInterface
import org.json.JSONArray
import org.json.JSONObject
import org.videolan.libvlc.LibVLC
import org.videolan.libvlc.Media
import org.videolan.libvlc.MediaPlayer

/**
 * Playback on Android, on libVLC's bundled FFmpeg.
 *
 * The webview's `<video>` is the wrong engine here. Chromium is built with
 * Dolby (AC-3, E-AC-3, TrueHD) and DTS switched off whatever the hardware
 * underneath can do, so a release carrying one plays as a picture with no
 * sound — on a TV box that decodes it in hardware, and even plugged into a
 * receiver that would have taken the bitstream untouched. libVLC ships its
 * own FFmpeg and decodes everything itself, so the same release plays with
 * full audio on the same device, with no transcode proxy on the Rust side.
 *
 * It answers the same command/property protocol mpv does (`player_ipc` /
 * `player_props`) and the same one `app/utils/htmlvideo.ts` speaks, so
 * `MpvPlayer.vue` drives all three backends through one code path. This
 * produces a picture, some sound and a clock; the page keeps drawing its
 * own controls, OSD and subtitle cues over the top.
 *
 * The picture is a TextureView directly below the WebView. The player page
 * makes only its video rectangle transparent, leaving the web controls above
 * the native frames and allowing its normal pointer handler to receive taps.
 *
 * `LibVLC` is created once on the main thread with no options (the bundled
 * defaults — hardware decoder on, software fallback via FFmpeg). The
 * `MediaPlayer` is created lazily on the first `start()` and re-used for
 * the lifetime of the activity. `release()` tears it down when the
 * activity is really going away.
 */
class RivuletPlayer(private val activity: MainActivity) {
  private val main = Handler(Looper.getMainLooper())

  private var lib: LibVLC? = null
  private var player: MediaPlayer? = null
  private var textureView: TextureView? = null
  private var videoSurface: Surface? = null
  private var outputAttached = false

  /** mpv keeps volume and mute apart, and the element has no mute at all. */
  private var vol = 100
  private var muted = false
  private var videoScaleMode = MediaPlayer.ScaleType.SURFACE_BEST_FIT

  /**
   * Whether the user explicitly paused, as opposed to libVLC pausing because
   * it is buffering. `paused-for-cache` must only be true when the player is
   * stalling for data, not when the user hit pause — otherwise the frontend
   * shows the loading spinner over the pause overlay.
   */
  @Volatile
  private var userPaused = false

  /** Last libVLC Buffering event, 0–100. mpv's `cache-buffering-state`. */
  @Volatile
  private var cacheFill = 0

  /**
   * Video outputs libVLC has open, from its `Vout` event. Some live HLS
   * channels play with no size on `currentVideoTrack` and a `time` that never
   * moves, so neither of the page's picture signals ever arrives and it sat on
   * "Connecting" over a playing channel. A vout is the one that always does.
   */
  @Volatile
  private var voutCount = 0

  /**
   * The last events libVLC sent, newest last, with the milliseconds since
   * `start()`. A channel that never opens produces no error at all —
   * `EncounteredError` is a decoder failure, and a stream that simply never
   * arrives fires nothing — so "Connecting…" sat on screen with nothing
   * anywhere to say why. This is that missing answer, and the player shows it.
   *
   * The URL is recorded without its query: a Free TV proxy URL carries the
   * upstream in `?url=`, and an Xtream one carries the account's password.
   */
  private val trace = java.util.concurrent.ConcurrentLinkedDeque<String>()

  @Volatile
  private var startedAt = 0L

  private fun note(what: String) {
    val at = if (startedAt == 0L) 0 else android.os.SystemClock.elapsedRealtime() - startedAt
    trace.addLast("${at}ms $what")
    while (trace.size > 24) trace.pollFirst()
    android.util.Log.d("RivuletPlayer", "trace ${at}ms $what")
  }

  /** Scheme, host, port and path only — never the query. */
  private fun safeUrl(url: String): String = url.substringBefore('?')

  /** No length is how libVLC says "live"; a file always reports one. */
  private fun isLiveStream(): Boolean = (player?.length ?: 0L) <= 0L

  /**
   * libVLC says the stream stopped. For a file that is the truth and the page
   * should hear it now. For live it is a maybe, so start the grace window and
   * let `refresh` decide after libVLC has had its own go at reconnecting.
   */
  private fun markDead(why: String?) {
    if (why != null) failure = why
    // Only a stream that has actually shown a picture gets the benefit of the
    // doubt. One that never produced a vout has not hiccuped, it has failed,
    // and Free TV's walk down the list must not wait out a grace window for it.
    if (isLiveStream() && voutCount > 0) {
      if (deadAt == 0L) deadAt = android.os.SystemClock.elapsedRealtime()
    } else {
      running = false
    }
  }

  @Volatile
  private var snap = JSONObject()

  @Volatile
  private var running = false

  @Volatile
  private var failure: String? = null

  /**
   * When libVLC last said this stream ended or errored, or 0.
   *
   * Live reports both spuriously: an HLS discontinuity raises EndReached,
   * `:http-reconnect` carries on, and the picture never actually stops.
   * `running` used to be one-way — set in `start`, cleared on the first
   * hiccup — so one of those left a channel marked stopped for the rest of
   * its life while it played on. The page then sat on "Connecting" and
   * started skipping down the list over a working picture. Live now gets a
   * grace window; a file, which has a length, still ends the moment it says so.
   */
  @Volatile
  private var deadAt = 0L

  /** How long a live stream may be silent before it has really stopped. */
  private val liveRecoverMs = 6_000L

  private val tick = object : Runnable {
    override fun run() {
      refresh()
      main.postDelayed(this, 100)
    }
  }

  // -------------------------------------------------------------------------
  // The protocol
  // -------------------------------------------------------------------------

  @JavascriptInterface
  fun start(url: String) {
    failure = null
    running = true
    deadAt = 0L
    userPaused = false
    cacheFill = 0
    voutCount = 0
    trace.clear()
    startedAt = android.os.SystemClock.elapsedRealtime()
    note("start ${safeUrl(url)}")
    onMain {
      val p = ensure()
      activity.setVlcVideoMode(true)
      textureView?.visibility = View.VISIBLE
      // `Media(lib, url)`'s constructor doesn't always take the URL through
      // libVLC's MRL parser — on some libVLC builds the constructor falls
      // back to `input-slave` parsing, which treats a URL with multiple
      // path segments as a relative file path and ends up prepending
      // `file:////` (the `https%3A//` in the MRL string is the smoking
      // gun — that's libVLC re-encoding the protocol of a URL it didn't
      // recognise as a network stream). `setLocation` + `parse` runs the
      // MRL through the same parser the standalone VLC client uses.
      val media = Media(lib, Uri.parse(url))
      // Hardware decoders on; libVLC falls back to FFmpeg itself when a
      // device's MediaCodec claim doesn't pan out (the very reason E-AC-3
      // is silent under ExoPlayer on a lot of cheap TV boxes).
      media.setHWDecoderEnabled(true, false)
      // Options have to be added before the media is handed to the player and
      // before it is released. Adding one afterwards calls into a freed native
      // object and is the release-build crash seen when opening a stream.
      // 4K HEVC IDR frames need more than 300ms or the decoder skips
      // them and the picture stays 1080p-soft. Hardware decode keeps
      // this from stalling start; skipping the loop filter / IDCT is
      // what made UHD look like a transcode.
      media.addOption(":network-caching=3000")
      media.addOption(":file-caching=1200")
      media.addOption(":live-caching=3000")
      // Keep every HEVC loop-filter / IDCT coefficient. The previous
      // skip=4 path is why Android 4K looked like a 720p transcode.
      media.addOption(":avcodec-skiploopfilter=0")
      media.addOption(":avcodec-skip-frame=0")
      media.addOption(":avcodec-skip-idct=0")
      // Debrid hosts reject libVLC's default UA; the proxy also sends this,
      // but a wrap miss used to hit the resolver with Lavf and sit 30s/hop.
      media.addOption(":http-user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
      media.addOption(":http-reconnect")
      media.addOption(":no-mediacodec-dr")
      p.media = media
      media.release()
      // Keep the page's mute/volume state when switching channels. This also
      // avoids leaving a reused MediaPlayer at volume zero after unmuting.
      p.volume = if (muted) 0 else vol
      p.play()
      main.removeCallbacks(tick)
      tick.run()
    }
  }

  @JavascriptInterface
  fun stop() {
    running = false
    deadAt = 0L
    voutCount = 0
    note("stop")
    onMain {
      main.removeCallbacks(tick)
      player?.stop()
      activity.setVlcVideoMode(false)
      // A TextureView left visible keeps its last frame painted, which is a
      // confusing thing to see when the next title hasn't started.
      textureView?.visibility = View.GONE
      snap = JSONObject()
    }
  }

  @JavascriptInterface
  fun command(json: String): String {
    val cmd = JSONArray(json)
    if (cmd.optString(0) == "set_property") {
      val name = cmd.optString(1)
      val value = cmd.opt(2)
      onMain { setProp(name, value) }
    } else if (cmd.optString(0) == "seek") {
      val amount = cmd.optDouble(1)
      val flags = cmd.optString(2)
      onMain { seek(amount, flags) }
    }
    // `sub-add` never reaches here: external subtitles are downloaded, parsed
    // and drawn by the page (utils/subtitles.ts), so the shim answers it itself.
    return "null"
  }

  @JavascriptInterface
  fun props(names: String): String {
    val want = JSONArray(names)
    val from = snap
    val out = JSONObject()
    for (i in 0 until want.length()) {
      val key = want.optString(i)
      // Absent rather than null for anything we can't produce, which is what mpv
      // does for a property it has no answer for.
      if (from.has(key)) out.put(key, from.get(key))
    }
    return out.toString()
  }

  @JavascriptInterface
  fun status(): String {
    val lines = JSONArray()
    for (line in trace) lines.put(line)
    val p = player
    return JSONObject()
      .put("running", running)
      .put("log_tail", failure ?: JSONObject.NULL)
      // What the player is actually doing, for the diagnostic the page shows
      // when a channel will not start. See `trace`.
      .put("trace", lines)
      .put("state", p?.playerState ?: -1)
      .put("vout", voutCount)
      .put("buffering", cacheFill)
      .put("time", if (p == null || p.time < 0) 0L else p.time)
      .toString()
  }

  /**
   * Every mime type this device can decode, straight from the platform.
   *
   * libVLC has its own decoder set, but `isAwkward` in utils/torrents.ts asks
   * this before demoting a release — a TV box almost always has E-AC-3 and
   * HEVC, a mid-range phone often has neither. MediaCodecList is the
   * platform's own record of what has a decoder, and the same heuristic mpv's
   * `track-list` builds on for the menu.
   */
  @JavascriptInterface
  fun codecs(): String {
    val out = JSONArray()
    for (info in MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos) {
      if (info.isEncoder) continue
      for (type in info.supportedTypes) out.put(type.lowercase())
    }
    return out.toString()
  }

  /**
   * Which video types this device decodes at 4K *in hardware*, and at 10-bit.
   *
   * `codecs()` answers "is there a decoder for this type", and for HEVC every
   * phone says yes: Google's software decoder ships with the platform. That
   * one cannot keep 3840×2160 in real time on a phone CPU — and `start` opens
   * media with `setHWDecoderEnabled(true, false)`, whose `false` lets libVLC
   * fall back to a software path of its own whenever the hardware decoder
   * turns a stream down. On a phone that plays as a frozen picture over audio
   * that keeps going. So the question for UHD is narrower: a hardware
   * decoder, that takes the size, with the 10-bit profile when the release is
   * 10-bit. `uhdPlayable` in htmlvideo.ts is what asks it.
   *
   * `{ "video/hevc": { "uhd": true, "uhd10": true }, … }`, with a type absent
   * when no hardware decoder takes it at 4K.
   *
   * Answered from a cache a background thread fills when this player is
   * created, and never worked out on the call. Every `@JavascriptInterface` on
   * a WebView runs on one shared JavaBridge thread, and the page blocks until
   * the call returns. Walking `MediaCodecList` and asking each decoder for its
   * capabilities — seconds, on some phones — in here held up the player's own
   * `start`, queued behind it on that thread, and a direct link that used to
   * open at once sat on Buffering. "" until the walk is done, or if it failed,
   * which the page reads as "unknown" and asks again.
   */
  @JavascriptInterface
  fun videoCaps(): String = uhdCaps ?: ""

  @Volatile
  private var uhdCaps: String? = null

  init {
    // Normal priority, on purpose. Android builds its decoder list once per
    // process under one lock, and libVLC's own decoder setup takes that lock
    // too. A background-priority thread holding it is starved for as long as
    // the app is busy — which, at the moment a film starts, it always is — and
    // everything waiting on the lock waits with it.
    Thread({
      uhdCaps = runCatching { computeVideoCaps() }.getOrNull()
    }, "RivuletVideoCaps").apply {
      isDaemon = true
      start()
    }
  }

  private fun computeVideoCaps(): String {
    val out = JSONObject()
    for (info in MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos) {
      if (info.isEncoder || !isHardwareDecoder(info)) continue
      for (type in info.supportedTypes) {
        val mime = type.lowercase()
        if (!mime.startsWith("video/")) continue
        val caps = runCatching { info.getCapabilitiesForType(type) }.getOrNull() ?: continue
        val video = caps.videoCapabilities ?: continue
        // Both orientations: some decoders state their limits portrait.
        val uhd = runCatching {
          video.isSizeSupported(3840, 2160) || video.isSizeSupported(2160, 3840)
        }.getOrDefault(false)
        if (!uhd) continue
        val entry = out.optJSONObject(mime) ?: JSONObject().put("uhd", true).put("uhd10", false)
        if (caps.profileLevels.any { isTenBitProfile(mime, it.profile) })
          entry.put("uhd10", true)
        out.put(mime, entry)
      }
    }
    return out.toString()
  }

  /**
   * API 29 says so outright. Before it, the platform's own software decoders
   * are recognisable by name — `OMX.google.*` on the old stack, `c2.android.*`
   * on Codec2 — and a vendor's hardware ones are not.
   */
  private fun isHardwareDecoder(info: android.media.MediaCodecInfo): Boolean {
    if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q)
      return info.isHardwareAccelerated && !info.isSoftwareOnly
    val name = info.name.lowercase()
    return !(name.startsWith("omx.google.") || name.startsWith("c2.android.") || name.contains("ffmpeg"))
  }

  /** HDR of every flavour is 10-bit, so the HDR profiles count too. */
  private fun isTenBitProfile(mime: String, profile: Int): Boolean = when (mime) {
    "video/hevc" -> profile == android.media.MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10 ||
      profile == android.media.MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10 ||
      profile == android.media.MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10Plus
    "video/x-vnd.on2.vp9" -> profile == android.media.MediaCodecInfo.CodecProfileLevel.VP9Profile2 ||
      profile == android.media.MediaCodecInfo.CodecProfileLevel.VP9Profile2HDR ||
      profile == android.media.MediaCodecInfo.CodecProfileLevel.VP9Profile2HDR10Plus
    "video/av01" -> profile == android.media.MediaCodecInfo.CodecProfileLevel.AV1ProfileMain10 ||
      profile == android.media.MediaCodecInfo.CodecProfileLevel.AV1ProfileMain10HDR10 ||
      profile == android.media.MediaCodecInfo.CodecProfileLevel.AV1ProfileMain10HDR10Plus
    "video/avc" -> profile == android.media.MediaCodecInfo.CodecProfileLevel.AVCProfileHigh10
    else -> false
  }

  fun release() {
    onMain {
      main.removeCallbacks(tick)
      player?.release()
      player = null
      videoSurface?.release()
      videoSurface = null
      outputAttached = false
      lib?.release()
      lib = null
      textureView?.let { view ->
        (view.parent as? ViewGroup)?.removeView(view)
      }
      textureView = null
      activity.setVlcVideoMode(false)
    }
  }

  // -------------------------------------------------------------------------
  // The player itself
  // -------------------------------------------------------------------------

  private fun ensure(): MediaPlayer {
    player?.let { return it }

    if (lib == null) {
      // Headless: no logging to stdout, no chroma override, hardware decoders
      // on. An `ArrayList` (not Kotlin's `emptyList()`, which is the read-only
      // `Collections.EMPTY_LIST`) — libVLC 3.4.0's constructor calls
      // `add(…)` on it and throws `UnsupportedOperationException` otherwise.
      //
      // Do not force OpenSL ES here. On several Android 13+ devices that
      // output module initialises successfully but produces silence. libVLC's
      // Android default selects AudioTrack/AAudio as appropriate for the
      // device and remains the safest route for APK builds.
      // TextureView + MediaCodec direct rendering is the usual 4K black
      // frame: the decoder "succeeds" and never delivers pixels. Keep HW
      // decode, but copy frames through the SurfaceTexture.
      lib = LibVLC(activity, arrayListOf("--no-stats", "--no-mediacodec-dr"))
    }

    val p = MediaPlayer(lib)

    p.setEventListener { event ->
      when (event.type) {
        MediaPlayer.Event.EncounteredError -> {
          markDead("libVLC could not open this stream.")
          note("EncounteredError")
        }
        MediaPlayer.Event.EndReached -> {
          markDead(null)
          note("EndReached")
        }
        MediaPlayer.Event.Buffering -> {
          val pct = event.buffering.toInt().coerceIn(0, 100)
          // Every buffering tick would be two dozen lines of noise; the ends
          // of the range are what say whether data is arriving at all.
          if (pct == 0 || pct >= 100 || cacheFill / 25 != pct / 25) note("Buffering $pct%")
          cacheFill = pct
        }
        MediaPlayer.Event.Paused -> note("Paused")
        MediaPlayer.Event.Opening -> note("Opening")
        MediaPlayer.Event.Stopped -> note("Stopped")
        MediaPlayer.Event.Playing, MediaPlayer.Event.Vout, MediaPlayer.Event.ESAdded -> {
          // Any of the three is proof the stream is alive. This is what makes
          // `running` recoverable: without it a single spurious EndReached
          // stopped the page's polling for good, and only a fresh `start`
          // (the Retry button) ever set it back.
          running = true
          deadAt = 0L
          failure = null
          if (event.type == MediaPlayer.Event.Playing) {
            cacheFill = 100
            note("Playing")
          }
          if (event.type == MediaPlayer.Event.Vout) {
            voutCount = event.voutCount
            note("Vout ${event.voutCount}")
          }
          if (event.type == MediaPlayer.Event.ESAdded) note("ESAdded")
          // Cover/stretch need the video track size; that only exists after
          // the first vout. `setVideoScale` is a no-op on a raw TextureView.
          updateVideoLayout()
        }
        MediaPlayer.Event.TimeChanged -> Unit
        else -> note("Event ${event.type}")
      }
    }

    // TextureView (not SurfaceView) is the right view for libVLC on Android.
    // A `SurfaceView` paints in its own window layer — `setZOrderOnTop(true)`
    // puts it above the WebView, which hides the OSD; without that flag, an
    // opaque WebView covers it and only the WebView's transparent parts (the
    // OSD) leave the video visible, so the picture shows up only as a
    // half-screen strip. TextureView is a normal view in the ViewGroup's
    // window; with it at index 0 and the WebView at index 1, the WebView
    // paints over it but its DOM is transparent in the player area
    // (MpvPlayer.vue's `html.rivulet-video { background: transparent }`), so
    // the video shows through and the OSD HTML sits on top of it like on
    // every other player.
    //
    // Tap forwarding: TextureView's `onTouchEvent` is invoked before the
    // WebView's, because TextureView at index 0 sits below it in the
    // ViewGroup. The forwarded event is dispatched onto the WebView, which
    // is where `MpvPlayer.vue`'s `tapVideo` handler reads it.
    val tv = TextureView(activity)
    // Player mode rotates a phone into landscape *after* this view can have
    // been created. libVLC retains its first window size unless it is told
    // about the new bounds, which leaves an old portrait-sized video surface
    // on the left and a black strip on the right.
    tv.addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ ->
      updateVideoLayout()
    }
    tv.surfaceTextureListener = object : TextureView.SurfaceTextureListener {
      override fun onSurfaceTextureAvailable(surface: SurfaceTexture, width: Int, height: Int) {
        videoSurface?.release()
        videoSurface = Surface(surface)
        outputAttached = false
        attachVideoOutput()
      }

      override fun onSurfaceTextureSizeChanged(surface: SurfaceTexture, width: Int, height: Int) {}

      override fun onSurfaceTextureDestroyed(surface: SurfaceTexture): Boolean {
        player?.vlcVout?.detachViews()
        outputAttached = false
        videoSurface?.release()
        videoSurface = null
        return true
      }

      override fun onSurfaceTextureUpdated(surface: SurfaceTexture) {}
    }
    tv.visibility = View.GONE
    // The WebView (index 1) sits on top of this TextureView (index 0).
    // Android's touch dispatch is top-to-bottom: the WebView sees the touch
    // first, turns it into a JS pointer event, and `MpvPlayer.vue`'s
    // `tapVideo` handler reads it. Nothing to forward.
    val params = ViewGroup.MarginLayoutParams(
      ViewGroup.LayoutParams.MATCH_PARENT,
      ViewGroup.LayoutParams.MATCH_PARENT,
    )
    activity.findViewById<ViewGroup>(android.R.id.content).addView(tv, 0, params)
    textureView = tv
    player = p
    // Usually the listener runs after this assignment, but Android may report
    // an already-created TextureView synchronously. Attach in both orders so
    // a fast route transition cannot leave a playing stream with no output.
    if (tv.isAvailable && videoSurface == null)
      videoSurface = Surface(tv.surfaceTexture)
    attachVideoOutput()

    return p
  }

  /** Attach libVLC to the current TextureView surface exactly once. */
  private fun attachVideoOutput() {
    val p = player ?: return
    val surface = videoSurface ?: return
    if (outputAttached) return
    p.vlcVout.setVideoSurface(surface, null)
    updateVideoLayout()
    p.vlcVout.attachViews()
    outputAttached = true
  }

  /** Keep libVLC's output dimensions in sync with the rotated TextureView. */
  private fun updateVideoLayout() {
    val p = player ?: return
    val view = textureView ?: return
    if (view.width <= 0 || view.height <= 0) return
    p.vlcVout.setWindowSize(view.width, view.height)
    applyVideoScale(p, view.width, view.height)
  }

  /**
   * Fit / center / stretch.
   *
   * `MediaPlayer.setVideoScale` only runs if a `VLCVideoLayout` created a
   * VideoHelper. This player attaches a raw TextureView under the webview
   * (so the OSD can paint over it), so that call is a no-op. VideoHelper
   * itself just turns the enum into `setScale` / `setAspectRatio` — do
   * the same here.
   */
  private fun applyVideoScale(p: MediaPlayer, viewW: Int, viewH: Int) {
    when (videoScaleMode) {
      MediaPlayer.ScaleType.SURFACE_BEST_FIT -> {
        p.setAspectRatio(null)
        p.setScale(0f)
      }
      MediaPlayer.ScaleType.SURFACE_FIT_SCREEN -> {
        val track = p.currentVideoTrack
        if (track == null || track.width <= 0 || track.height <= 0) {
          p.setAspectRatio(null)
          p.setScale(0f)
          return
        }
        val rotated = track.orientation == 5 || track.orientation == 6
        var vw = if (rotated) track.height else track.width
        val vh = if (rotated) track.width else track.height
        if (track.sarNum != track.sarDen && track.sarDen != 0)
          vw = vw * track.sarNum / track.sarDen
        if (vw <= 0 || vh <= 0) {
          p.setAspectRatio(null)
          p.setScale(0f)
          return
        }
        val videoAr = vw.toFloat() / vh
        val viewAr = viewW.toFloat() / viewH
        p.setScale(if (viewAr >= videoAr) viewW.toFloat() / vw else viewH.toFloat() / vh)
        p.setAspectRatio(null)
      }
      else -> {
        p.setScale(0f)
        p.setAspectRatio("$viewW:$viewH")
      }
    }
  }

  private fun setProp(name: String, value: Any?) {
    val p = player ?: return
    when (name) {
      "pause" -> {
        userPaused = value == true
        if (value == true) p.pause() else p.play()
      }
      "time-pos" -> p.setTime((num(value, 0.0) * 1000).toLong())
      "volume" -> {
        vol = num(value, 100.0).toInt().coerceIn(0, 100)
        muted = false
        p.volume = vol
      }
      "mute" -> {
        muted = value == true
        p.volume = if (muted) 0 else vol
      }
      "speed" -> p.setRate(num(value, 1.0).toFloat().coerceAtLeast(0.1f))
      "video-scale" -> {
        val mode = value?.toString() ?: "contain"
        videoScaleMode = when (mode) {
          "cover" -> MediaPlayer.ScaleType.SURFACE_FIT_SCREEN
          "fill" -> MediaPlayer.ScaleType.SURFACE_FILL
          else -> MediaPlayer.ScaleType.SURFACE_BEST_FIT
        }
        updateVideoLayout()
      }
      // Track selection. libVLC's `setAudioTrack(int)` / `setSpuTrack(int)`
      // take a track id (the values reported in `getAudioTrack()` / `getSpuTrack()`).
      // The page asks by `track-list` id; we map the negative "no track" case
      // to -1, which libVLC also uses internally.
      "aid" -> {
        val want = num(value, 0.0).toInt()
        p.setAudioTrack(if (want <= 0) -1 else audioIdForListIndex(want - 1))
      }
      "sid" -> {
        val want = num(value, 0.0).toInt()
        p.setSpuTrack(if (want <= 0) -1 else subIdForListIndex(want - 1))
      }
    }
  }

  /**
   * Same flags as mpv's `seek`. Percent-seek is how live jumps to the edge:
   * a live window has no useful duration, so 100% is `setPosition(1)`.
   */
  private fun seek(amount: Double, flags: String) {
    val p = player ?: return
    when {
      flags.contains("absolute-percent") ->
        p.setPosition((amount / 100.0).toFloat().coerceIn(0f, 1f))
      flags.contains("absolute") ->
        p.setTime((amount * 1000).toLong())
      else ->
        p.setTime(p.time + (amount * 1000).toLong())
    }
  }

  /**
   * One pass over everything the page polls, built on the main thread so the
   * bridge can answer from any other one.
   */
  private fun refresh() {
    val p = player ?: return

    // A live stream that said it stopped and then stayed quiet really has
    // stopped. One that came back on its own already cleared this above.
    val dead = deadAt
    if (dead != 0L && android.os.SystemClock.elapsedRealtime() - dead > liveRecoverMs) {
      deadAt = 0L
      if (!p.isPlaying) {
        running = false
        note("no picture after the grace window - giving up")
      }
    }

    val audioTracks = p.audioTracks ?: emptyArray()
    val spuTracks = p.spuTracks ?: emptyArray()
    val list = JSONArray()
    var aid: Any = "no"
    var sid: Any = "no"
    val currentAudioId = p.audioTrack
    val currentSpuId = p.spuTrack

    // Numbered sequentially for the page — the 1-based indices it puts in
    // `track-list` and the way `set_property aid/sid` round-trip.
    var audioIdx = 0
    for (track in audioTracks) {
      audioIdx++
      list.put(
        JSONObject()
          .put("id", audioIdx)
          .put("type", "audio")
          .put("lang", JSONObject.NULL)
          .put("title", track.name ?: JSONObject.NULL),
      )
      if (currentAudioId == track.id) aid = audioIdx
    }
    var spuIdx = 0
    for (track in spuTracks) {
      spuIdx++
      list.put(
        JSONObject()
          .put("id", audioIdx + spuIdx)
          .put("type", "sub")
          .put("lang", JSONObject.NULL)
          .put("title", track.name ?: JSONObject.NULL),
      )
      if (currentSpuId == track.id) sid = audioIdx + spuIdx
    }

    val length = p.length
    val duration = if (length <= 0) 0.0 else length / 1000.0
    val pos = if (p.time < 0) 0.0 else p.time / 1000.0
    val rate = p.rate.toDouble()
    // Opening a Direct URL reports pause and length=0. `pos < duration` is
    // then false, so the page thought we were idle and hid Loading.
    val stalling = !p.isPlaying && !userPaused && (length <= 0 || pos < duration)
    val track = p.currentVideoTrack
    snap = JSONObject()
      .put("pause", !p.isPlaying)
      .put("paused-for-cache", stalling)
      .put("duration", duration)
      .put("time-pos", pos)
      .put("demuxer-cache-time", pos)
      .put("cache-buffering-state", cacheFill)
      .put("volume", vol)
      .put("mute", muted)
      .put("speed", rate)
      .put("track-list", list)
      .put("aid", aid)
      .put("sid", sid)
      .put("sub-text", "")
      // mpv's name for "a video output is up" — see `voutCount`.
      .put("vo-configured", voutCount > 0)
    if (track != null && track.width > 0 && track.height > 0) {
      snap.put(
        "video-params",
        JSONObject().put("w", track.width).put("h", track.height),
      )
    }
  }

  /** Convert a 1-based audio index from the page into libVLC's track id. */
  private fun audioIdForListIndex(idx: Int): Int {
    val p = player ?: return -1
    val tracks = p.audioTracks ?: return -1
    if (idx < 0 || idx >= tracks.size) return -1
    return tracks[idx].id
  }

  /** Convert a 1-based subtitle index from the page into libVLC's track id. */
  private fun subIdForListIndex(idx: Int): Int {
    val p = player ?: return -1
    val tracks = p.spuTracks ?: return -1
    if (idx < 0 || idx >= tracks.size) return -1
    return tracks[idx].id
  }

  private fun num(value: Any?, fallback: Double) = (value as? Number)?.toDouble() ?: fallback

  private fun onMain(block: () -> Unit) {
    if (Looper.myLooper() == Looper.getMainLooper()) block() else main.post(block)
  }
}
