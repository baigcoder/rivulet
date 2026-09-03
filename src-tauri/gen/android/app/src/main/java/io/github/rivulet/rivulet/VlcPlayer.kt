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

  @Volatile
  private var snap = JSONObject()

  @Volatile
  private var running = false

  @Volatile
  private var failure: String? = null

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
    userPaused = false
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
      media.addOption(":network-caching=300")
      media.addOption(":no-mediacodec-dr")
      // Hardware decoders on with software fallback. For 4K content the
      // hardware decoder may hit its resolution ceiling; FFmpeg picks up
      // the frames it cannot handle. `avcodec-fast` disables certain
      // quality features that are expensive on a phone-sized SoC, and
      // skipping the loop filter shaves enough CPU for 4K on mid-range
      // chips without visible quality loss at viewing distance.
      media.addOption(":avcodec-fast")
      media.addOption(":avcodec-skiploopfilter=4")
      media.addOption(":avcodec-skipidct=4")
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
  fun status(): String =
    JSONObject().put("running", running).put("log_tail", failure ?: JSONObject.NULL).toString()

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
          running = false
          failure = "libVLC could not play this file."
          android.util.Log.e("RivuletPlayer", "EncounteredError — decoder failed or codec unsupported")
        }
        MediaPlayer.Event.EndReached -> running = false
        MediaPlayer.Event.Paused -> android.util.Log.d("RivuletPlayer", "Paused")
        MediaPlayer.Event.Playing -> android.util.Log.d("RivuletPlayer", "Playing")
        MediaPlayer.Event.TimeChanged -> Unit
        else -> android.util.Log.d("RivuletPlayer", "Event: ${event.type}")
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
    // Same default as the page (contain / best-fit). Rotation is handled in
    // `updateVideoLayout`, which reapplies the mode the user last picked.
    p.setVideoScale(videoScaleMode)
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
    p.setVideoScale(videoScaleMode)
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
        val mode = value?.toString() ?: "fill"
        videoScaleMode = when (mode) {
          "contain" -> MediaPlayer.ScaleType.SURFACE_BEST_FIT
          "cover" -> MediaPlayer.ScaleType.SURFACE_FILL
          else -> MediaPlayer.ScaleType.SURFACE_FIT_SCREEN
        }
        p.setVideoScale(videoScaleMode)
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
    snap = JSONObject()
      .put("pause", !p.isPlaying)
      .put("paused-for-cache", !p.isPlaying && !userPaused && pos < duration)
      .put("duration", duration)
      .put("time-pos", pos)
      .put("demuxer-cache-time", pos)
      .put("volume", vol)
      .put("mute", muted)
      .put("speed", rate)
      .put("track-list", list)
      .put("aid", aid)
      .put("sid", sid)
      .put("sub-text", "")
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
