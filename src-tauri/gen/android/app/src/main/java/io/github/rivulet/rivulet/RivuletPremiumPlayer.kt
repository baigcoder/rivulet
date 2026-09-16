package io.github.rivulet.rivulet

import android.graphics.SurfaceTexture
import android.os.Handler
import android.os.Looper
import android.view.Surface
import android.view.TextureView
import android.view.View
import android.view.ViewGroup
import android.webkit.JavascriptInterface
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.hls.HlsMediaSource
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.exoplayer.source.MediaSource
import org.json.JSONArray
import org.json.JSONObject

/**
 * Premium TV live playback on Android, on Media3 ExoPlayer.
 *
 * Same shape as `RivuletPlayer` (libVLC, VlcPlayer.kt) but
 * Media3 instead — the HLS path is the same primitive the
 * web player uses, and Media3's HLS implementation ships with
 * the runtime so no extra FFmpeg is required. The webview's
 * Chromium demuxer is Dolby-blind on the cheap set-top boxes
 * the app runs on; Media3 falls back to platform MediaCodec
 * where it can, which is what the rest of the app gets when
 * the source is already an HLS manifest.
 *
 * The protocol the page sees is identical to `RivuletPlayer`:
 *
 *   - `start(url)` — load + play
 *   - `stop()`     — pause + release the surface
 *   - `command(json)` — set_property / sub-add (sub-add is a
 *     no-op here: external subtitles are drawn by the page,
 *     see utils/subtitles.ts)
 *   - `props(names)` — JSON snapshot of the named properties
 *   - `status()`  — JSON `{running, log_tail}`
 *
 * The picture is a TextureView directly below the WebView, at
 * index 0 in the same content view. WebView's CSS transparency
 * shows the video through, the DOM draws the OSD on top.
 */
class RivuletPremiumPlayer(private val activity: MainActivity) {
    private val main = Handler(Looper.getMainLooper())

    private var player: ExoPlayer? = null
    private var textureView: TextureView? = null
    private var videoSurface: Surface? = null
    private var outputAttached = false

    /**
     * A newly-added TextureView does not have a SurfaceTexture until Android's
     * next layout pass. Starting Media3 before then is usually fine on a warm
     * player, but on the first channel it can leave the decoder with no output
     * and the web UI waits for a first frame forever. Keep the start pending
     * until the surface arrives, just as the libVLC player does.
     */
    private var pendingPlay = false
    private val surfaceWaitMs = 3_000L
    private val playWhenReady = Runnable {
        if (pendingPlay) {
            // Orientation and system-bar changes can take longer than this on
            // a cold Android start. Starting headless is not a fallback: it is
            // the first-attempt bug. Leave the start pending for the surface
            // callback, which is the only safe place to begin rendering.
            android.util.Log.d("RivuletPremiumPlayer", "still waiting for video surface")
        }
    }

    @Volatile
    private var running = false

    @Volatile
    private var failure: String? = null

    @Volatile
    private var snap = JSONObject()

    @Volatile
    private var vol = 100

    @Volatile
    private var muted = false

    /**
     * Whether a frame has actually been drawn.
     *
     * Media3 says this outright: `onRenderedFirstFrame` is the moment a picture
     * exists, which is a stronger signal than libVLC's video-output count and
     * is exactly what the page keys "is it playing" on. Reported as
     * `vo-configured`, the same name mpv uses, so the page needs to know
     * nothing about which engine answered.
     */
    @Volatile
    private var firstFrame = false

    /** Media3's own starved-for-data state, for `paused-for-cache`. */
    @Volatile
    private var buffering = false

    /**
     * When Media3 last said this stream ended or errored, or 0.
     *
     * Live reports both spuriously: a discontinuity in an HLS or MPEG-TS feed
     * raises STATE_ENDED and the picture never actually stops. `running` was
     * one-way — set in `start`, cleared on the first hiccup — so one of those
     * marked a channel stopped for the rest of its life while it played on.
     *
     * The page reads `running` to decide whether to keep polling at all, so
     * what the viewer saw was everything downstream of a poll that had given
     * up: the HUD stuck on "Opening the stream…" over a moving picture, the
     * one-shot auto-retry firing eight seconds in and restarting a channel
     * that was working, and the auto-skip walking down the list past channels
     * that were all playing. Retry appeared to fix it because a fresh start is
     * the only thing that ever set this back.
     *
     * VlcPlayer.kt has had this grace since the same bug was found there. A
     * stream with a real length still ends the moment it says so.
     */
    @Volatile
    private var deadAt = 0L

    /** How long a live stream may be silent before it has really stopped. */
    private val liveRecoverMs = 6_000L

    /** No length is how a live stream reports itself; a file always has one. */
    private fun isLiveStream(): Boolean = (player?.duration ?: 0L) <= 0L

    /**
     * Media3 says the stream stopped. For a file that is the truth. For live it
     * is a maybe, so start the grace window and let `refresh` decide once
     * Media3 has had its own go at carrying on.
     *
     * Only a stream that has actually shown a picture gets the benefit of the
     * doubt. One that never rendered a frame has not hiccuped, it has failed,
     * and Free TV's walk down the list must not wait out a grace window for it.
     */
    private fun markDead(why: String?) {
        if (why != null) failure = why
        if (isLiveStream() && firstFrame) {
            if (deadAt == 0L) deadAt = android.os.SystemClock.elapsedRealtime()
        } else {
            running = false
        }
    }

    private val tick = object : Runnable {
        override fun run() {
            refresh()
            // The page polls at 200ms; rebuilding faster than it is read is
            // work taken out of the frame budget for nothing. See VlcPlayer.kt.
            main.postDelayed(this, 200)
        }
    }

    // ── Protocol ─────────────────────────────────────────────

    @JavascriptInterface
    fun start(url: String) {
        failure = null
        running = true
        firstFrame = false
        buffering = false
        deadAt = 0L
        onMain {
            val p = ensure()
            activity.setVlcVideoMode(true)
            textureView?.visibility = View.VISIBLE
            val httpFactory = DefaultHttpDataSource.Factory()
                .setUserAgent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
                .setConnectTimeoutMs(6_000)
                .setReadTimeoutMs(10_000)
                .setAllowCrossProtocolRedirects(true)
            val dataSourceFactory = DefaultDataSource.Factory(activity, httpFactory)
            // HLS only when the stream is HLS. This class was written for
            // Premium TV, where "the source is already an HLS manifest" holds,
            // and it forced `HlsMediaSource` on whatever it was handed. Once
            // live TV in general was routed here that stopped being true: a
            // Free TV channel is usually raw MPEG-TS behind the loopback proxy,
            // and asking the HLS parser to read a transport stream as a playlist
            // fails on the first bytes — which reached the page as a channel
            // that never opened, and never as a reason.
            val source: MediaSource = if (looksLikeHls(url))
                HlsMediaSource.Factory(dataSourceFactory)
                    .createMediaSource(MediaItem.fromUri(url))
            else
                DefaultMediaSourceFactory(dataSourceFactory)
                    .createMediaSource(MediaItem.fromUri(url))
            p.setMediaSource(source)
            p.prepare()
            p.volume = if (muted) 0f else vol / 100f
            // Do not ask the decoder for its first frame until it has somewhere
            // to draw it. A retry works only because that surface happens to
            // exist by then; the first attempt must wait for it itself.
            p.playWhenReady = false
            if (outputAttached) {
                p.playWhenReady = true
            } else {
                pendingPlay = true
                main.removeCallbacks(playWhenReady)
                main.postDelayed(playWhenReady, surfaceWaitMs)
            }
            main.removeCallbacks(tick)
            tick.run()
        }
    }

    @JavascriptInterface
    fun stop() {
        running = false
        firstFrame = false
        buffering = false
        onMain {
            main.removeCallbacks(tick)
            pendingPlay = false
            main.removeCallbacks(playWhenReady)
            player?.stop()
            activity.setVlcVideoMode(false)
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
        }
        return "null"
    }

    @JavascriptInterface
    fun props(names: String): String {
        val want = JSONArray(names)
        val from = snap
        val out = JSONObject()
        for (i in 0 until want.length()) {
            val key = want.optString(i)
            if (from.has(key)) out.put(key, from.get(key))
        }
        return out.toString()
    }

    /** A line from the page into logcat. See `androidLog` and VlcPlayer.kt. */
    @JavascriptInterface
    fun log(line: String) {
        android.util.Log.d("RivuletPremiumPlayer", "js $line")
    }

    @JavascriptInterface
    fun status(): String =
        JSONObject().put("running", running).put("log_tail", failure ?: JSONObject.NULL).toString()

    fun release() {
        onMain {
            main.removeCallbacks(tick)
            pendingPlay = false
            main.removeCallbacks(playWhenReady)
            player?.release()
            player = null
            videoSurface?.release()
            videoSurface = null
            textureView?.let { view ->
                (view.parent as? ViewGroup)?.removeView(view)
            }
            textureView = null
            activity.setVlcVideoMode(false)
        }
    }

    // ── Player setup ────────────────────────────────────────

    /**
     * Does this URL name an HLS playlist?
     *
     * Free TV plays through the loopback proxy, whose own path says nothing
     * about the stream — the real address is the `url=` parameter, so that is
     * what gets read. Everything else (an Xtream `…/id.ts`, a bare progressive
     * link) is not HLS and must not be handed to the HLS parser.
     */
    private fun looksLikeHls(url: String): Boolean {
        var target = url
        val q = url.indexOf('?')
        if (q >= 0) {
            for (pair in url.substring(q + 1).split('&')) {
                val (k, v) = pair.split('=', limit = 2).let {
                    it[0] to (it.getOrNull(1) ?: "")
                }
                if (k == "url" && v.isNotEmpty()) {
                    target = try {
                        java.net.URLDecoder.decode(v, "UTF-8")
                    } catch (_: Exception) {
                        v
                    }
                    break
                }
            }
        }
        val path = target.substringBefore('#').substringBefore('?').lowercase()
        return path.endsWith(".m3u8") || path.endsWith(".m3u")
    }

    private fun ensure(): ExoPlayer {
        player?.let { return it }

        val loadControl = androidx.media3.exoplayer.DefaultLoadControl.Builder()
            .setBufferDurationsMs(
                1500,
                15000,
                500,
                1000
            )
            .build()

        val p = ExoPlayer.Builder(activity)
            .setLoadControl(loadControl)
            .build()
        p.addListener(object : Player.Listener {
            override fun onPlayerError(error: androidx.media3.common.PlaybackException) {
                markDead(error.message ?: "Media3 could not play this stream.")
                android.util.Log.e("RivuletPremiumPlayer", "error: ${error.message}")
            }
        })
        p.addListener(object : Player.Listener {
            override fun onPlaybackStateChanged(state: Int) {
                buffering = state == Player.STATE_BUFFERING
                when (state) {
                    Player.STATE_ENDED -> markDead(null)
                    // Anything that is not the end is Media3 carrying on, and
                    // that retires a grace window the hiccup before it opened.
                    Player.STATE_READY, Player.STATE_BUFFERING -> {
                        running = true
                        deadAt = 0L
                        failure = null
                    }
                }
            }

            /** The picture exists. See `firstFrame`. */
            override fun onRenderedFirstFrame() {
                firstFrame = true
                buffering = false
                running = true
                deadAt = 0L
                failure = null
                android.util.Log.d("RivuletPremiumPlayer", "trace first frame")
            }
        })

        val tv = TextureView(activity)
        tv.surfaceTextureListener = object : TextureView.SurfaceTextureListener {
            override fun onSurfaceTextureAvailable(surface: SurfaceTexture, width: Int, height: Int) {
                videoSurface?.release()
                videoSurface = Surface(surface)
                p.setVideoSurface(videoSurface)
                outputAttached = true
                if (pendingPlay) {
                    pendingPlay = false
                    main.removeCallbacks(playWhenReady)
                    android.util.Log.d("RivuletPremiumPlayer", "surface ready; starting")
                    p.playWhenReady = true
                }
            }

            override fun onSurfaceTextureSizeChanged(surface: SurfaceTexture, width: Int, height: Int) {}

            override fun onSurfaceTextureDestroyed(surface: SurfaceTexture): Boolean {
                // A rotation destroys this surface and builds another, and the
                // decoder loses its output with the first one. Handing it the
                // replacement is not on its own enough to get it rendering
                // again, so the start is made pending exactly as a cold one is
                // — unless the viewer paused on purpose, or the stream was
                // stopped, in which case there is nothing to resume.
                //
                // VlcPlayer.kt has carried this since the day live TV worked at
                // all; live TV moved onto this player without it, and the
                // symptom came back word for word: the picture goes black on
                // rotate, the HUD says it is connecting, and Retry looks like
                // the fix when all Retry does is run once the rotation has
                // finished.
                //
                // `p.playWhenReady` alone over-reaches: this rotation is
                // entering player mode itself, landing in the same instant as
                // the tap most viewers make on a screen that is still
                // loading — the centre button is the only thing there to tap.
                // If no frame has ever rendered (`!firstFrame`) there is
                // nothing paused to preserve, only an open that never got to
                // happen; honouring the pause there is the "opening never
                // finishes, Retry fixes it" report, because Retry's only
                // advantage is a fresh `start()` that resets `playWhenReady`
                // itself.
                if (running && (p.playWhenReady || !firstFrame))
                    pendingPlay = true
                // And there is no picture until the new surface has drawn one.
                // The page reads this back as `vo-configured` to decide whether
                // a channel is playing, so leaving it true describes a picture
                // that is not on screen and cannot come back on its own.
                firstFrame = false
                p.clearVideoSurface()
                outputAttached = false
                videoSurface?.release()
                videoSurface = null
                return true
            }

            override fun onSurfaceTextureUpdated(surface: SurfaceTexture) {}
        }
        tv.visibility = View.GONE
        val params = ViewGroup.MarginLayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.MATCH_PARENT,
        )
        activity.findViewById<ViewGroup>(android.R.id.content).addView(tv, 0, params)
        textureView = tv
        player = p
        // Usually the listener runs after the view is added, but Android may
        // hand us an already-created TextureView synchronously. Attach in both
        // orders so that fast first navigation cannot wait forever for a
        // callback that already happened.
        if (tv.isAvailable && videoSurface == null) {
            videoSurface = Surface(tv.surfaceTexture)
            p.setVideoSurface(videoSurface)
            outputAttached = true
        }
        return p
    }

    private fun setProp(name: String, value: Any?) {
        val p = player ?: return
        when (name) {
            "pause" -> p.playWhenReady = value != true
            "time-pos" -> { /* live streams aren't seekable */ }
            "volume" -> {
                vol = (value as? Number)?.toDouble()?.toInt()?.coerceIn(0, 100) ?: 100
                if (!muted) p.volume = vol / 100f
            }
            "mute" -> {
                muted = value == true
                p.volume = if (muted) 0f else vol / 100f
            }
            "speed" -> {
                p.setPlaybackSpeed((value as? Number)?.toDouble()?.toFloat()?.coerceAtLeast(0.1f) ?: 1.0f)
            }
        }
    }

    private fun refresh() {
        val p = player ?: return

        // A live stream that said it stopped and then stayed quiet really has
        // stopped. One that came back on its own already cleared this above.
        val dead = deadAt
        if (dead != 0L && android.os.SystemClock.elapsedRealtime() - dead > liveRecoverMs) {
            deadAt = 0L
            if (!p.isPlaying) {
                running = false
                android.util.Log.d("RivuletPremiumPlayer", "no picture after the grace window - giving up")
            }
        }
        val duration = if (p.duration <= 0) 0.0 else p.duration / 1000.0
        val pos = if (p.currentPosition < 0) 0.0 else p.currentPosition / 1000.0
        val format = p.videoFormat
        // No invented size. It used to report 1280x720 whenever a stream was
        // playing without a format yet, which is a resolution the page then
        // showed the viewer. `vo-configured` is how "there is a picture" is
        // answered now, so the size can simply be absent until it is known.
        val vw = format?.width ?: 0
        val vh = format?.height ?: 0
        snap = JSONObject()
            // Paused means the viewer paused it. Media3's `isPlaying` is false
            // through every rebuffer as well — and a live stream rebuffers
            // constantly — so reporting that as `pause` dropped the page out of
            // "playing" several times a minute: the chrome stopped auto-hiding
            // and the connecting overlay came back over a channel that was on
            // screen. `playWhenReady` is the intent, which is what mpv's
            // `pause` means; starvation is `paused-for-cache` below.
            .put("pause", !p.playWhenReady)
            // Media3 says outright whether it is starved. The old test was
            // `pos < duration`, and live reports no duration — so it was always
            // false and the page could never learn it was waiting on data.
            .put("paused-for-cache", buffering)
            .put("cache-buffering-state", if (buffering) 0 else 100)
            .put("duration", duration)
            .put("time-pos", pos)
            .put("demuxer-cache-time", pos)
            .put("volume", vol)
            .put("mute", muted)
            .put("speed", p.playbackParameters.speed.toDouble())
            .put("vo-configured", firstFrame)
        if (vw > 0 && vh > 0) {
            snap.put(
                "video-params",
                JSONObject().put("w", vw).put("h", vh),
            )
        }
        if (p.isPlaying) {
            snap.put(
                "audio-params",
                JSONObject().put("samplerate", 48000).put("channel-count", 2),
            )
        }
    }

    private fun onMain(block: () -> Unit) {
        if (Looper.myLooper() == Looper.getMainLooper()) block() else main.post(block)
    }
}
