/**
 * A whole-second countdown that ends in one action — the "Next episode in 15s"
 * the player shows once the credits roll. `remaining` is null while idle, so a
 * template can show the countdown with `v-if` and nothing else. Starting one
 * that is already running is ignored, and it stops with its component.
 */
export function useCountdown(onDone: () => void) {
  const remaining = ref<number | null>(null)
  let handle: ReturnType<typeof setTimeout> | null = null

  function clear() {
    remaining.value = null
    if (handle) {
      clearTimeout(handle)
      handle = null
    }
  }

  function tick() {
    if (remaining.value === null)
      return
    remaining.value--
    if (remaining.value <= 0) {
      clear()
      onDone()
      return
    }
    handle = setTimeout(tick, 1000)
  }

  function start(seconds: number) {
    if (remaining.value !== null)
      return
    remaining.value = seconds
    handle = setTimeout(tick, 1000)
  }

  onScopeDispose(clear)
  return { remaining, start, clear }
}
