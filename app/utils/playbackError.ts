/**
 * Many IPTV panels answer a full connection count with a short error
 * video* — white text on grey, often muxed as a subtitle track — rather
 * than HTTP 403. mpv plays it like any other file until something reads
 * the cue and stops.
 */
export function isProviderConnectionLimit(raw: string | null | undefined): boolean {
  const lower = (raw ?? '').toLowerCase()
  return lower.includes('max connection')
    || lower.includes('connection limit')
    || lower.includes('too many connection')
    || lower.includes('simultaneous connection')
    || lower.includes('exceeded the maximum')
    || lower.includes('mutiple login') // common panel typo
    || lower.includes('multiple login')
    || lower.includes('already watching')
    || lower.includes('active connection')
    || lower.includes('keep trying to exceed')
    || lower.includes('multiple devices')
    || lower.includes('more then allowed')
    || lower.includes('more than allowed')
}

/** Premium VOD under two minutes is a panel error clip, not a film. */
export function isProviderVodSlateDuration(secs: number): boolean {
  return secs > 0 && secs < 120
}

/** One sentence when the panel says every slot is taken. */
export function connectionLimitMessage(active?: number | null, max?: number | null): string {
  if (active != null && max != null && max > 0) {
    return $t('Your provider is at its connection limit ({active} of {max} streams in use). Stop playback on your other devices, then try again.', {
      active,
      max,
    })
  }
  return $t('Your provider is at its connection limit. Stop playback on your other devices, then try again.')
}

/**
 * Turn mpv/ffmpeg/proxy noise into one sentence a viewer can act on.
 * The Rust side redacts credentials from log tails, but hostnames and
 * decoder lines still read like a crash dump — they never belong on screen.
 */
export function friendlyPlaybackError(raw: string | null | undefined): string {
  const s = (raw ?? '').trim()
  if (!s)
    return $t('Playback failed. Try again or pick another title.')

  if (isProviderConnectionLimit(s))
    return connectionLimitMessage()

  const lower = s.toLowerCase()

  if (
    lower.includes('name or service not known')
    || lower.includes('failed to resolve hostname')
    || lower.includes('nodename nor servname')
    || lower.includes('no address associated')
  ) {
    return $t('This channel\'s server could not be reached. It may be offline — try another channel.')
  }

  if (
    lower.includes('connection refused')
    || lower.includes('connection reset')
    || lower.includes('timed out')
    || lower.includes('timeout')
  ) {
    return $t('The stream timed out or refused the connection. Try again in a moment.')
  }

  if (
    /\b401\b/.test(lower)
    || /\b403\b/.test(lower)
    || lower.includes('unauthorized')
    || lower.includes('forbidden')
  ) {
    return $t('The provider refused this stream. Your account may be at its connection limit.')
  }

  if (/\b404\b/.test(lower) || lower.includes('not found'))
    return $t('This stream is no longer available from the provider.')

  if (/\b502\b/.test(lower) || /\b503\b/.test(lower) || lower.includes('bad gateway'))
    return $t('The stream server is not responding. Try another channel.')

  if (
    lower.includes('[ffmpeg]')
    || lower.includes('[stream]')
    || lower.includes('failed to open http')
    || lower.includes('failed to open https')
    || lower.includes('protocol not found')
    || lower.includes('invalid data found')
  ) {
    return $t('This stream could not be opened. Try another channel or check your connection.')
  }

  // Already a translated app sentence — pass through.
  if (!lower.includes('http://') && !lower.includes('https://') && s.length < 160)
    return s

  return $t('Playback failed. Try again or pick another title.')
}

/** Format seconds as m:ss or h:mm:ss for the live HUD. */
export function fmtHudTime(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0)
    return '0:00'
  const t = Math.floor(secs)
  const h = Math.floor(t / 3600)
  const m = Math.floor((t % 3600) / 60)
  const sec = t % 60
  if (h > 0)
    return `${h}:${String(m).padStart(2, '0')}:${String(sec).padStart(2, '0')}`
  return `${m}:${String(sec).padStart(2, '0')}`
}
