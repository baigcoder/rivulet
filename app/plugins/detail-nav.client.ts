/** Snapshot the pressed card before the title page mounts — no backdrop, no theme. */
export default defineNuxtPlugin(() => {
  const router = useRouter()

  router.beforeEach(to => {
    const ui = useUiStore()
    // A title keeps `opening` for the splash; people/season routes must not inherit it.
    if (!isTitlePath(to.path))
      ui.opening = null

    const { type, id } = to.params as Record<string, string | string[]>
    if ((type !== 'movie' && type !== 'tv') || !id)
      return
    const armedMedia = takeArmed(type as MediaType, String(id))
    const media = armedMedia ?? peekMediaDetail(type as MediaType, String(id))
    if (media)
      ui.open(media)
  })

  // Home keeps the shell scrolled. The title page mounts in the same box, so
  // without this the cover sits above the fold and seasons clip under the bar.
  router.afterEach(to => {
    if (!isTitlePath(to.path))
      return
    const reset = () => {
      const shell = document.querySelector('[data-dpad-start]')
      if (shell instanceof HTMLElement)
        shell.scrollTop = 0
    }
    reset()
    requestAnimationFrame(reset)
  })
})
