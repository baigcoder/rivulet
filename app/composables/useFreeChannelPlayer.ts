import type { LiveChannel } from '~/utils/iptv'
import { saveLivePlay } from '~/utils/liveNav'

/**
 * Open a Free TV channel in the player, with `list` as the lineup up/down
 * walks — the order the viewer was just reading, whichever screen that was.
 * Free TV's grid and the Live TV landing's rows both start playback here, so
 * the two can't drift apart on what the player is handed.
 */
export function useFreeChannelPlayer() {
  const liveTv = useLiveTvStore()
  const router = useRouter()
  const route = useRoute()

  /** "undefined"/"null" are real values in a public M3U. */
  const playable = (c: LiveChannel) => !!c.streamUrl && c.streamUrl !== 'undefined' && c.streamUrl !== 'null'

  function play(ch: LiveChannel, list: LiveChannel[]): void {
    if (!playable(ch))
      return
    liveTv.rememberChannel(ch.id)
    // It lives on the store so a zap is a small query change, not a
    // 60-channel JSON rewrite.
    const zapList = list
      .filter(playable)
      .slice(0, 200)
      .map(c => ({
        id: c.id,
        name: c.name,
        logoUrl: c.logoUrl,
        streamUrl: c.streamUrl,
        userAgent: c.userAgent,
        referer: c.referer,
      }))
    liveTv.setZapList(zapList)
    const sourceId = liveTv.activeSourceId || 'free:iptv-org'
    saveLivePlay({
      id: ch.id,
      title: ch.name,
      logo: ch.logoUrl ?? '',
      sourceId,
      // `playable` above has already turned an empty one away.
      streamUrl: ch.streamUrl ?? '',
      userAgent: ch.userAgent,
      referer: ch.referer,
      zapList,
    })
    // Identity only — the stream URL stays on the zap list. Putting it in
    // the query broke on `&` in the path and blew the address bar up.
    void router.push({
      path: localePath('/live-tv/watch'),
      query: {
        id: ch.id,
        title: ch.name,
        logo: ch.logoUrl ?? '',
        type: 'live',
        sourceId,
        from: route.fullPath,
      },
    })
  }

  return { play }
}
