import type { Reminder } from '~/utils/reminders'
import { isPermissionGranted, requestPermission } from '@tauri-apps/plugin-notification'
import { createGlobalState, useLocalStorage } from '@vueuse/core'
import { ref } from 'vue'

/**
 * The reminders set, shared by every bell and the plugin that rings them.
 * `rivulet.` prefixed, so a backup carries them with everything else.
 */
export const useReminders = createGlobalState(() => {
  const list = useLocalStorage<Reminder[]>('rivulet.liveTv.reminders', [])
  /** Rung and not yet dismissed: what the in-app toast is showing. */
  const ringing = ref<Reminder[]>([])

  function remove(key: string): void {
    list.value = list.value.filter(r => r.key !== key)
  }

  function has(key: string): boolean {
    return list.value.some(r => r.key === key)
  }

  function toggle(r: Reminder): void {
    if (has(r.key)) {
      remove(r.key)
      return
    }
    list.value = [...list.value, r].sort((a, b) => a.start - b.start)
    // Asked here, inside the press, rather than when it rings: Android 13+
    // shows nothing without it, and a prompt out of nowhere a minute before
    // a programme is the worst moment to ask.
    void isPermissionGranted()
      .then(async granted => {
        if (!granted)
          await requestPermission()
      })
      .catch(() => { /* no Tauri: the in-app toast still rings */ })
  }

  return { list, ringing, has, toggle, remove }
})
