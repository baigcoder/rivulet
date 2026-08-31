export default defineNuxtPlugin(() => {
  if (import.meta.server)
    return

  function onKeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault()
      const el = document.querySelector<HTMLInputElement>('[data-live-search]')
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
