<script setup lang="ts">
/**
 * A reminder that just rang, with Watch.
 *
 * Deliberately not a `v-snackbar`: that is a Vuetify overlay, and the d-pad
 * treats every active overlay as a modal (`dpad.client.ts`) — a reminder
 * landing in the middle of a film would take the remote away, and Back,
 * which a snackbar does not answer, would do nothing until it timed out. A
 * fixed panel with two real buttons is reachable and ignorable alike.
 */
import type { LiveChannel } from '~/utils/iptv'
import type { Reminder } from '~/utils/reminders'
import { mdiBellRing, mdiPlay } from '@mdi/js'
import { onUnmounted, watch } from 'vue'

/** Long enough to cross a room for; a rung reminder is no use after. */
const SHOW_MS = 60_000

const { ringing } = useReminders()
const router = useRouter()
const { play: playFree } = useFreeChannelPlayer()

const current = computed(() => ringing.value[0] ?? null)

function dismiss(): void {
  ringing.value = ringing.value.slice(1)
}

let timer: ReturnType<typeof setTimeout> | undefined
watch(current, r => {
  clearTimeout(timer)
  if (r)
    timer = setTimeout(dismiss, SHOW_MS)
}, { immediate: true })
onUnmounted(() => clearTimeout(timer))

function watchNow(r: Reminder): void {
  dismiss()
  if (r.kind === 'premium') {
    void router.push({ path: localePath('/live-tv/premium/watch'), query: { id: r.channelId } })
    return
  }
  if (!r.stream)
    return
  const ch = {
    id: r.channelId,
    name: r.channelName,
    logoUrl: r.stream.logoUrl,
    streamUrl: r.stream.url,
    userAgent: r.stream.userAgent,
    referer: r.stream.referer,
  } as LiveChannel
  playFree(ch, [ch])
}

function when(r: Reminder): string {
  const mins = Math.round((r.start - Date.now()) / 60_000)
  return mins > 0 ? $t('Starts in {minutes} min on {channel}', { minutes: mins, channel: r.channelName }) : $t('Now on {channel}', { channel: r.channelName })
}
</script>

<template>
  <div
    v-if="current"
    class="fixed bottom-4 end-4 z-[2400] flex w-[min(32rem,calc(100vw-2rem))] items-center gap-3 rounded-2xl bg-surface-container-highest p-4 shadow-xl ring-1 ring-white/10"
    role="status"
    aria-live="polite"
  >
    <v-icon :icon="mdiBellRing" size="26" class="shrink-0 text-primary" />
    <div class="min-w-0 flex-1">
      <p class="truncate text-title-small font-semibold">
        {{ current.title }}
      </p>
      <p class="truncate text-body-medium opacity-70">
        {{ when(current) }}
      </p>
    </div>
    <button
      type="button"
      class="flex h-10 shrink-0 items-center gap-1.5 rounded-xl bg-primary px-3 text-label-large font-semibold text-on-primary transition-[filter] hover:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white"
      @click="watchNow(current)"
    >
      <v-icon :icon="mdiPlay" size="18" />
      {{ $t('Watch') }}
    </button>
    <button
      type="button"
      class="h-10 shrink-0 rounded-xl px-3 text-label-large font-medium opacity-80 transition-colors hover:bg-white/10 focus-visible:bg-white/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
      @click="dismiss"
    >
      {{ $t('Dismiss') }}
    </button>
  </div>
</template>
