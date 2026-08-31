<script setup lang="ts">
/**
 * The provider status strip: who is connected, how much of their catalog
 * is on disk, and how old it is.
 *
 * Everything here comes off `/status`, which is a SQLite read — no
 * provider is contacted to draw this. The `username` is shown because the
 * user typed it and it identifies which of their accounts is live; the
 * password is not on this wire at all (see `models.rs`, where it is
 * `skip_serializing`).
 */
import type { CatalogState, PremiumAccount } from '~/types/premium'
import { mdiAccountCircle, mdiLogout, mdiRefresh } from '@mdi/js'
import { computed } from 'vue'

const props = defineProps<{
  account: PremiumAccount
  catalog: CatalogState | null
  busy?: boolean
}>()

const emit = defineEmits<{
  refresh: []
  disconnect: []
}>()

const label = computed(() =>
  props.account.accountName?.trim() || props.account.username || $t('Premium TV'),
)

const kind = computed(() =>
  props.account.providerType === 'm3u' ? $t('M3U / M3U+') : $t('Xtream Codes'),
)

/**
 * Xtream reports expiry as a Unix-seconds string; an M3U has no notion of
 * one. Anything that does not parse is dropped rather than shown raw —
 * "Expires 1735689600" is worse than no line.
 */
const expiry = computed(() => {
  const raw = props.account.expiresAt?.trim()
  if (!raw)
    return ''
  const secs = Number(raw)
  const date = Number.isFinite(secs) && secs > 0 ? new Date(secs * 1000) : new Date(raw)
  if (Number.isNaN(date.getTime()))
    return ''
  return date.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' })
})

const connections = computed(() => {
  const max = props.account.maxConnections
  if (!max)
    return ''
  return `${props.account.activeConnections ?? 0}/${max}`
})

/** "12 minutes ago" beats a timestamp for the one thing it answers. */
function ago(secs?: number | null): string {
  if (!secs)
    return ''
  const mins = Math.floor((Date.now() / 1000 - secs) / 60)
  if (mins < 1)
    return $t('just now')
  if (mins < 60)
    return $t('{minutes} min ago', { minutes: mins })
  const hours = Math.floor(mins / 60)
  if (hours < 24)
    return $t('{hours} h ago', { hours })
  return $t('{days} d ago', { days: Math.floor(hours / 24) })
}

const catalogAge = computed(() => ago(props.catalog?.catalogSyncedAt))
const epgAge = computed(() => ago(props.catalog?.epgSyncedAt))
const syncing = computed(() => props.catalog?.syncing === true)
</script>

<template>
  <section class="flex flex-wrap items-center gap-x-6 gap-y-3 rounded-2xl bg-surface-container px-4 py-3 ring-1 ring-white/6">
    <div class="flex min-w-0 items-center gap-3">
      <v-icon :icon="mdiAccountCircle" size="28" class="shrink-0 text-primary" />
      <div class="min-w-0">
        <p class="truncate text-title-small font-semibold">
          {{ label }}
        </p>
        <p class="truncate text-label-small opacity-55">
          {{ kind }}
          <template v-if="account.isTrial">
            · {{ $t('Trial') }}
          </template>
          <template v-if="expiry">
            · {{ $t('Expires {date}', { date: expiry }) }}
          </template>
          <template v-if="connections">
            · {{ $t('{connections} connections', { connections }) }}
          </template>
        </p>
      </div>
    </div>

    <dl v-if="catalog" class="flex flex-wrap items-center gap-x-5 gap-y-1 text-label-small">
      <div class="flex items-baseline gap-1.5">
        <dt class="opacity-45">
          {{ $t('Channels') }}
        </dt>
        <dd class="font-semibold tabular-nums">
          {{ catalog.channels.toLocaleString() }}
        </dd>
      </div>
      <div class="flex items-baseline gap-1.5">
        <dt class="opacity-45">
          {{ $t('Categories') }}
        </dt>
        <dd class="font-semibold tabular-nums">
          {{ catalog.categories.toLocaleString() }}
        </dd>
      </div>
      <div v-if="catalogAge" class="flex items-baseline gap-1.5">
        <dt class="opacity-45">
          {{ $t('Catalog') }}
        </dt>
        <dd class="opacity-75">
          {{ catalogAge }}
        </dd>
      </div>
      <!-- No guide, no row: a provider without XMLTV should not be shown
           an "EPG: never" line on every visit. -->
      <div v-if="epgAge" class="flex items-baseline gap-1.5">
        <dt class="opacity-45">
          {{ $t('Guide') }}
        </dt>
        <dd class="opacity-75">
          {{ epgAge }}
        </dd>
      </div>
    </dl>

    <div class="ms-auto flex items-center gap-1">
      <v-btn
        variant="text"
        size="small"
        :prepend-icon="mdiRefresh"
        :loading="busy || syncing"
        @click="emit('refresh')"
      >
        {{ $t('Refresh') }}
      </v-btn>
      <v-btn
        variant="text"
        size="small"
        color="error"
        :prepend-icon="mdiLogout"
        :disabled="busy"
        @click="emit('disconnect')"
      >
        {{ $t('Disconnect') }}
      </v-btn>
    </div>
  </section>
</template>
