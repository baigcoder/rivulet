/**
 * One reading of the player, shared by both live pages.
 *
 * Free TV and Premium TV mirror the same component into the same shape and
 * then draw very different chrome over it. Keeping two copies of that reading
 * is what produced most of the bugs in this area: `started` was dropped from
 * the playing test on one page and left on the other for four releases; the
 * stale-error rule was fixed on one and not the other; the failure handler
 * learned to defer to the picture on one and not the other. Every one of those
 * reached a viewer as the same complaint — the connecting card sitting over a
 * channel that was playing, with the chrome stuck open behind it.
 *
 * So the reading lives here and the pages keep only what is genuinely theirs:
 * Premium its reconnect machine, Free TV its auto-skip.
 */
import { ref } from 'vue'

/** What both pages need off `<mpv-player>`; each may expose more. */
export interface PlayerHandle {
  moving?: boolean | { value?: boolean }
  paused?: boolean | { value?: boolean }
  behindLive?: boolean | { value?: boolean }
  volume?: number
  muted?: boolean | { value?: boolean }
  ui?: boolean | { value?: boolean }
  position?: number
  duration?: number
  errorMsg?: string | { value?: string }
  catchError?: string | { value?: string }
}

/**
 * Exposed refs arrive unwrapped through `defineExpose`, except where a page
 * holds the component instance itself — so both shapes are accepted rather
 * than one being assumed and read as `undefined` on the day it is not.
 */
export function asBool(v: boolean | { value?: boolean } | undefined): boolean {
  if (v && typeof v === 'object' && 'value' in v)
    return !!v.value
  return !!v
}

export function asText(v: string | { value?: string } | undefined): string {
  if (v && typeof v === 'object' && 'value' in v)
    return String(v.value ?? '')
  return typeof v === 'string' ? v : ''
}

export function usePlayerMirror() {
  /** Frames on screen. The ground truth every other decision defers to. */
  const hasPicture = ref(false)
  const playerPlaying = ref(false)
  /**
   * The viewer asked for this stop.
   *
   * Kept apart from `playerPlaying`, which carries the pause inside it and so
   * reads the same for a channel someone stopped and one that never opened.
   * Both pages then said "connecting" over a deliberate pause — and on Android
   * within two seconds of it, because libVLC reports no size for a live track
   * and the only reading left is a clock that a pause stops.
   */
  const playerPaused = ref(false)
  const playerBehindLive = ref(false)
  const playerVolume = ref(100)
  const playerMuted = ref(false)
  const playerChrome = ref(false)
  const playerPosition = ref(0)
  const playerDuration = ref(0)
  const playerCatchError = ref('')

  /**
   * Read the player once. Returns whether a picture arrived on this pass, so a
   * page can do its own work on that edge without keeping a second copy of the
   * previous value.
   */
  function mirror(p: PlayerHandle): { picture: boolean, gainedPicture: boolean } {
    const hadPicture = hasPicture.value
    // `moving` in MpvPlayer, and nothing else — a video output that is up, or
    // a clock that is going forward without one. libVLC often reports no size
    // at all for a live channel on Android, which is why the size was never
    // enough on its own.
    //
    // It is not evidence either, which is what this used to treat it as. Both
    // Android backends publish the size the *stream* declares as soon as the
    // container is parsed, and both keep their TextureView hidden until a
    // frame is really drawn — so a premium channel that opened, said 720p and
    // then rendered nothing read here as a picture. Everything defers to this
    // flag, so that one reading took down the connecting panel, the start
    // watchdog, the one-shot retry, the auto-skip and the reconnect at once,
    // and left a black screen with nothing able to act on it.
    const picture = asBool(p.moving)
    hasPicture.value = picture
    // A picture that is not paused is playing. `started` was in this test on
    // the Premium page until v0.6.47 and is the page's own bookkeeping, not a
    // fact about the stream.
    playerPaused.value = asBool(p.paused)
    playerPlaying.value = picture && !playerPaused.value
    playerBehindLive.value = asBool(p.behindLive)
    playerVolume.value = typeof p.volume === 'number' ? p.volume : 100
    playerMuted.value = asBool(p.muted)
    playerChrome.value = asBool(p.ui)
    playerPosition.value = typeof p.position === 'number' ? p.position : 0
    playerDuration.value = typeof p.duration === 'number' ? p.duration : 0

    // A start-up log line must not sit over a stream that is running. Keyed on
    // the picture rather than on `playerPlaying`, which carries `paused` with
    // it and the live backends get `paused` wrong; the clock covers a film
    // whose first frame has not been reported yet.
    if (picture || playerPosition.value > 0.5)
      playerCatchError.value = ''
    else
      playerCatchError.value = asText(p.errorMsg ?? p.catchError)

    return { picture, gainedPicture: picture && !hadPicture }
  }

  return {
    hasPicture,
    playerPlaying,
    playerPaused,
    playerBehindLive,
    playerVolume,
    playerMuted,
    playerChrome,
    playerPosition,
    playerDuration,
    playerCatchError,
    mirror,
  }
}
