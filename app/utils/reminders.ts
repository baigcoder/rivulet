/**
 * "Remind me" for a programme that has not started. Pure, so the timing is
 * checkable without a clock; the state lives in `useReminders` and the
 * ringing in `plugins/reminders.client.ts`.
 */

export interface Reminder {
  key: string
  kind: 'free' | 'premium'
  channelId: string
  channelName: string
  title: string
  /** Epoch milliseconds. */
  start: number
  /**
   * Free TV only: what the player needs to start the channel without
   * looking it up again. A public stream address, never a credential —
   * a Premium reminder has none, and its channel is resolved by id.
   */
  stream?: { url: string, userAgent?: string | null, referer?: string | null, logoUrl?: string | null }
}

/** How long before the start a reminder rings. */
export const REMINDER_LEAD_MS = 60_000

/** How late a reminder still rings: the app may have been busy, or asleep. */
const GRACE_MS = 5 * 60_000

export function reminderKey(kind: Reminder['kind'], channelId: string, start: number): string {
  return `${kind}:${channelId}:${start}`
}

export function dueReminders(list: Reminder[], now: number): Reminder[] {
  return list.filter(r => r.start - REMINDER_LEAD_MS <= now && now < r.start + GRACE_MS)
}

export function pruneReminders(list: Reminder[], now: number): Reminder[] {
  return list.filter(r => now < r.start + GRACE_MS)
}
