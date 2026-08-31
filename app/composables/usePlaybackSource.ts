import type { PlaybackSource } from '~/types/premium'
import { onBeforeUnmount, ref } from 'vue'
import { premiumApi, PremiumApiError } from '~/utils/premiumTv'

/**
 * Race-safe source resolution for the Premium TV player.
 *
 * Zapping A → B → C puts three requests in flight before the first
 * answers, and each answer is a *signed, short-lived* redirector token —
 * so a late one is not merely stale, it is a token for the wrong channel
 * that would play. Two guards, because they stop different things: the
 * `AbortController` cancels the network work, and the request id stops a
 * response that was already decoding when the abort landed from being
 * assigned.
 *
 * The URL this returns is always `http://127.0.0.1:3032/premium-stream/…`
 * — never a provider URL. The Rust side answers it with a 302 to the
 * upstream after re-checking the entitlement, which is what keeps the
 * provider's host, path and per-account token out of the page entirely.
 */
export function usePlaybackSource() {
  const source = ref<PlaybackSource | null>(null)
  const loading = ref(false)
  const error = ref('')
  const channelId = ref<string | null>(null)

  let controller: AbortController | null = null
  let requestId = 0

  async function load(id: string): Promise<void> {
    if (controller)
      controller.abort()
    const own = new AbortController()
    controller = own
    const reqId = ++requestId
    channelId.value = id
    loading.value = true
    error.value = ''
    source.value = null
    try {
      const next = await premiumApi.play(id, own.signal)
      if (reqId !== requestId)
        return
      source.value = next
    }
    catch (e) {
      if (reqId !== requestId)
        return
      if (e instanceof DOMException && e.name === 'AbortError')
        return
      // Only the Rust side's own message is shown. It is written for a
      // user and is contractually free of credentials; a raw `fetch`
      // failure against loopback means the API is down, which "Failed to
      // fetch" does not tell anybody.
      error.value = e instanceof PremiumApiError
        ? e.message
        : $t('Premium TV is not responding. Try restarting the app.')
    }
    finally {
      if (reqId === requestId)
        loading.value = false
    }
  }

  function clear(): void {
    if (controller)
      controller.abort()
    controller = null
    requestId++
    source.value = null
    loading.value = false
    error.value = ''
    channelId.value = null
  }

  onBeforeUnmount(() => {
    if (controller)
      controller.abort()
  })

  return { source, loading, error, channelId, load, clear }
}
