import { sendNotification } from '@tauri-apps/plugin-notification'
import { dueReminders, pruneReminders } from '~/utils/reminders'

/** Often enough that a one-minute lead is never missed by more than half. */
const CHECK_MS = 30_000

/**
 * Rings reminders while the app runs: an OS notification, which reaches
 * someone in another window, and the in-app toast (`LiveReminderToast`),
 * which has the Watch button. Nothing rings while Rivulet is closed — there
 * is no background service for it, and on Android a backgrounded app is
 * frozen anyway — which the Live TV settings say in so many words.
 */
export default defineNuxtPlugin(() => {
  const { list, ringing, remove } = useReminders()

  function check(): void {
    const now = Date.now()
    for (const r of dueReminders(list.value, now)) {
      remove(r.key)
      ringing.value = [...ringing.value, r]
      try {
        sendNotification({
          title: r.start > now ? $t('Starting soon on {channel}', { channel: r.channelName }) : $t('Now on {channel}', { channel: r.channelName }),
          body: r.title,
        })
      }
      catch { /* no Tauri (a browser dev server): the toast still shows */ }
    }
    // What rang late enough to be useless is forgotten rather than kept.
    const kept = pruneReminders(list.value, now)
    if (kept.length !== list.value.length)
      list.value = kept
  }

  check()
  setInterval(check, CHECK_MS)
})
