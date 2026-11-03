import { ref } from 'vue'

/**
 * The channel the viewer is on: the last card focused (remote) or pointed at
 * (mouse) inside a grid. Read off `data-channel-id` from one listener on the
 * grid's container, so the virtualized grids pass nothing up through props.
 * A page mixing services marks each row with `data-channel-source`, since a
 * Free and a Premium id are not guaranteed to differ.
 */
export function useFeaturedChannel() {
  const featuredId = ref<string | null>(null)
  const featuredSource = ref<string | null>(null)
  function track(e: Event): void {
    const card = (e.target as HTMLElement | null)?.closest<HTMLElement>('[data-channel-id]')
    const id = card?.dataset.channelId
    if (!card || !id)
      return
    featuredId.value = id
    featuredSource.value = card.closest<HTMLElement>('[data-channel-source]')?.dataset.channelSource ?? null
  }
  return { featuredId, featuredSource, track }
}
