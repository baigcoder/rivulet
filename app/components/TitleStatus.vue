<script setup lang="ts">
import type { Release } from '~/utils/torrents'
import { mdiAlertCircleOutline, mdiCheckCircle, mdiCloudCheckOutline, mdiMagnet, mdiPowerPlugOutline } from '@mdi/js'

/**
 * What Play is going to find, said before Play is pressed: already on the
 * disk, instant from a cloud source, a torrent and how healthy it is, or
 * nothing at all. The title page already asks the sources ahead
 * (`searchAhead`) so Play can skip the search; this reads that same answer
 * without using it up, so it costs no request of its own.
 *
 * Only ever a statement about the user's own sources — with none added it
 * says so and points at the page where they are added, and never at a source.
 */
const props = defineProps<{
  progressKey: string
  imdbId?: string | null
  season?: number
  episode?: number
}>()

const settings = useSettingsStore()
const downloads = useDownloadsStore()

type State
  = | { kind: 'offline' }
    | { kind: 'checking' }
    | { kind: 'instant', quality: string, source: string }
    | { kind: 'torrent', quality: string, seeders: number }
    | { kind: 'none', reason: string }
    | { kind: 'no-sources' }

const found = ref<Release[] | null>(null)
const waiting = ref(false)

// A string key, so the watcher only runs again when one of these actually
// changes — a fresh tuple every time `activeSources` was rebuilt restarted it
// on each source report, and the badge never stopped "checking".
watch(() => `${props.imdbId}|${props.season ?? 0}|${props.episode ?? 0}|${settings.activeSources.length}`, async (_key, _old, onCleanup) => {
  const imdb = props.imdbId
  const season = props.season
  const episode = props.episode
  found.value = null
  waiting.value = false
  if (!imdb)
    return
  let live = true
  onCleanup(() => {
    live = false
  })
  // The page starts the same search, and which watcher runs first is not
  // ours to decide: when this got there first it found nothing and showed
  // nothing. Asking is free — `searchAhead` keeps one search per title.
  if (!downloads.cachedFor(props.progressKey))
    searchAhead(imdb, season ?? 0, episode ?? 0)
  const pending = peekAhead(imdb, season ?? 0, episode ?? 0)
  if (!pending)
    return
  waiting.value = true
  // Never "Checking…" for good: past the search's own per-source leash, the
  // badge stops waiting and says nothing was found rather than spinning on.
  const giveUp = new Promise<Release[]>(resolve => setTimeout(resolve, 25_000, []))
  const list = await Promise.race([pending.catch(() => [] as Release[]), giveUp])
  if (!live)
    return
  found.value = list
  waiting.value = false
}, { immediate: true })

/**
 * What a source said when it had nothing — "TorBox is currently unavailable!"
 * is worth more than "found nothing", since it says whose problem it is and
 * that it will pass. Only a report from this search's lifetime counts.
 */
const sourceSaid = computed(() => {
  const since = Date.now() - 3 * 60_000
  for (const base of settings.activeSources) {
    const meta = settings.sourceMeta[base]
    if (meta?.error && (meta.at ?? 0) > since)
      return meta.name ? `${meta.name}: ${meta.error}` : meta.error
  }
  return ''
})

const state = computed<State | null>(() => {
  if (downloads.cachedFor(props.progressKey))
    return { kind: 'offline' }
  if (!settings.activeSources.length)
    return { kind: 'no-sources' }
  if (waiting.value)
    return { kind: 'checking' }
  const list = found.value
  if (!list)
    return null
  if (!list.length)
    return { kind: 'none', reason: sourceSaid.value }
  const instant = settings.allowTorrents ? null : pickPlay(list)
  if (instant?.url)
    return { kind: 'instant', quality: instant.quality, source: settings.sourceMeta[instant.via ?? '']?.name ?? '' }
  const best = pickBest(list, undefined, false, true)
  return best ? { kind: 'torrent', quality: best.quality, seeders: best.seeders } : { kind: 'none', reason: sourceSaid.value }
})

const view = computed(() => {
  const s = state.value
  switch (s?.kind) {
    case 'offline':
      return { icon: mdiCheckCircle, tone: 'good', text: $t('Downloaded · plays offline') }
    case 'checking':
      return { icon: '', tone: 'muted', text: $t('Checking your sources…') }
    case 'instant':
      return { icon: mdiCloudCheckOutline, tone: 'good', text: [$t('Instant'), s.quality, s.source].filter(Boolean).join(' · ') }
    case 'torrent':
      return {
        icon: mdiMagnet,
        tone: s.seeders >= 20 ? 'good' : 'warn',
        text: [s.quality, $t('{count} seeders', { count: s.seeders })].filter(Boolean).join(' · '),
      }
    case 'none':
      return { icon: mdiAlertCircleOutline, tone: 'warn', text: s.reason || $t('Your sources found nothing for this') }
    case 'no-sources':
      return { icon: mdiPowerPlugOutline, tone: 'muted', text: $t('Add a source to play') }
    default:
      return null
  }
})
</script>

<template>
  <component
    :is="state?.kind === 'no-sources' ? 'nuxt-link' : 'div'"
    v-if="view"
    :to="state?.kind === 'no-sources' ? localePath('/settings/sources') : undefined"
    class="inline-flex items-center gap-2 self-start rounded-full border px-3 py-1 text-label-large font-medium outline-none focus-visible:ring-2 focus-visible:ring-primary"
    :class="{
      'border-emerald-400/40 bg-emerald-400/10 text-emerald-300': view.tone === 'good',
      'border-amber-400/40 bg-amber-400/10 text-amber-300': view.tone === 'warn',
      'border-white/20 bg-white/5 text-white/75': view.tone === 'muted',
    }"
    role="status"
  >
    <v-progress-circular v-if="state?.kind === 'checking'" indeterminate size="14" width="2" />
    <v-icon v-else :icon="view.icon" size="16" />
    {{ view.text }}
  </component>
</template>
