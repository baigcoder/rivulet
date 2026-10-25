import type { Ref } from 'vue'

/**
 * The player's A-B loop: mark A, mark B, and playback jumps back to A each
 * time it reaches B. The player owns the clock and the seek, so it passes in
 * where the playhead is and how to say things on screen, and checks `abLoop`
 * against the position itself.
 */
export function useAbLoop(options: {
  started: Ref<boolean>
  position: Ref<number>
  osd: (text: string) => void
  fmt: (seconds: number) => string
}) {
  const { started, position, osd, fmt } = options
  /** null = not looping, { a, b } = loop between these two points. */
  const abLoop = ref<{ a: number, b: number } | null>(null)

  function setLoopA() {
    if (!started.value)
      return
    abLoop.value = { a: position.value, b: abLoop.value?.b ?? position.value + 10 }
    osd($t('Loop A: {time}', { time: fmt(position.value) }))
  }

  function setLoopB() {
    if (!started.value || !abLoop.value)
      return
    if (position.value <= abLoop.value.a) {
      abLoop.value = null
      osd($t('Loop cancelled'))
      return
    }
    abLoop.value.b = position.value
    osd($t('Loop: {a} → {b}', { a: fmt(abLoop.value.a), b: fmt(position.value) }))
  }

  function clearLoop() {
    abLoop.value = null
    osd($t('Loop off'))
  }

  return { abLoop, setLoopA, setLoopB, clearLoop }
}
