import { ref } from 'vue'

/**
 * The channel the viewer is on: the last card focused (remote) or pointed at
 * (mouse) inside a grid. Read off `data-channel-id` from one listener on the
 * grid's container, so the virtualized grids pass nothing up through props.
 */
export function useFeaturedChannel() {
  const featuredId = ref<string | null>(null)
  function track(e: Event): void {
    const id = (e.target as HTMLElement | null)?.closest<HTMLElement>('[data-channel-id]')?.dataset.channelId
    if (id && id !== featuredId.value)
      featuredId.value = id
  }
  return { featuredId, track }
}
