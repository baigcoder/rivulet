import { scheme, sourceFromImage } from '~/theme/palette'

/**
 * One colour picked from a title's own artwork, as the page's accent — so a
 * horror film's page is not the same teal as a cartoon's. The app-wide "follow
 * the art" setting recolours everything; this colours one page and touches
 * nothing else.
 *
 * Read off a w300 copy fetched CORS-clean for the purpose (the hero's own
 * w780 was asked for without CORS, and the browser would refuse its pixels),
 * and pushed through Material's generator for a dark-surface tone, so the
 * colour is always light enough to read on the hero. '' until it is known, and
 * for good where the host refuses CORS — the theme's primary stands in.
 */
export function useArtAccent(path: MaybeRefOrGetter<string | null | undefined>) {
  const accent = ref('')
  watch(() => toValue(path), (file, _old, onCleanup) => {
    accent.value = ''
    if (!file || import.meta.server)
      return
    let live = true
    onCleanup(() => {
      live = false
    })
    const image = new Image()
    image.crossOrigin = 'anonymous'
    image.onload = () => {
      const source = live ? sourceFromImage(image) : null
      if (source)
        accent.value = scheme(source, true).primary ?? ''
    }
    image.src = `${backdropUrl(file, 'w780')?.replace('/w780/', '/w300/')}?accent=1`
  }, { immediate: true })
  return accent
}
