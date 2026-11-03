export default defineNuxtPlugin(() => {
  if (import.meta.server)
    return

  function onKeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault()
      // The attribute sits on SearchField's wrapper; the input is inside it.
      const el = document.querySelector<HTMLInputElement>('[data-live-search] input')
      if (el) {
        if (document.activeElement === el) {
          el.blur()
        }
        else {
          el.focus()
          el.select()
        }
      }
    }
  }

  window.addEventListener('keydown', onKeydown)
})
