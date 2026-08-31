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

    private val tick = object : Runnable {
        override fun run() {
            refresh()
            main.postDelayed(this, 100)
        }
    }

    // ── Protocol ─────────────────────────────────────────────

    @JavascriptInterface
    fun start(url: String) {
        failure = null
        running = true
        onMain {
            val p = ensure()
            activity.setVlcVideoMode(true)
            textureView?.visibility = View.VISIBLE
            val httpFactory = DefaultHttpDataSource.Factory()
                .setUserAgent("Rivulet/0.5 (Media3)")
                .setConnectTimeoutMs(10_000)
                .setReadTimeoutMs(15_000)
                .setAllowCrossProtocolRedirects(true)
            val dataSourceFactory = DefaultDataSource.Factory(activity, httpFactory)
            val source: MediaSource = HlsMediaSource.Factory(dataSourceFactory)
                .createMediaSource(MediaItem.fromUri(url))
            p.setMediaSource(source)
            p.prepare()
            p.volume = if (muted) 0f else vol / 100f
            p.playWhenReady = true
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

    @JavascriptInterface
    fun status(): String =
        JSONObject().put("running", running).put("log_tail", failure ?: JSONObject.NULL).toString()

    fun release() {
        onMain {
            main.removeCallbacks(tick)
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

    private fun ensure(): ExoPlayer {
        player?.let { return it }

        val p = ExoPlayer.Builder(activity).build()
        p.addListener(object : Player.Listener {
            override fun onPlayerError(error: androidx.media3.common.PlaybackException) {
                running = false
                failure = error.message ?: "Media3 could not play this stream."
                android.util.Log.e("RivuletPremiumPlayer", "error: ${error.message}")
            }
        })
        p.addListener(object : Player.Listener {
            override fun onPlaybackStateChanged(state: Int) {
                when (state) {
                    Player.STATE_ENDED -> running = false
                    Player.STATE_READY, Player.STATE_BUFFERING -> Unit
                }
            }
        })

        val tv = TextureView(activity)
        tv.surfaceTextureListener = object : TextureView.SurfaceTextureListener {
            override fun onSurfaceTextureAvailable(surface: SurfaceTexture, width: Int, height: Int) {
                videoSurface = Surface(surface)
                p.setVideoSurface(videoSurface)
            }

            override fun onSurfaceTextureSizeChanged(surface: SurfaceTexture, width: Int, height: Int) {}

            override fun onSurfaceTextureDestroyed(surface: SurfaceTexture): Boolean {
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
        val duration = if (p.duration <= 0) 0.0 else p.duration / 1000.0
        val pos = if (p.currentPosition < 0) 0.0 else p.currentPosition / 1000.0
        snap = JSONObject()
            .put("pause", !p.isPlaying)
            .put("paused-for-cache", !p.isPlaying && pos < duration)
            .put("duration", duration)
            .put("time-pos", pos)
            .put("volume", vol)
            .put("mute", muted)
            .put("speed", p.playbackParameters.speed.toDouble())
    }

    private fun onMain(block: () -> Unit) {
        if (Looper.myLooper() == Looper.getMainLooper()) block() else main.post(block)
    }
}
