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

const serverHost = computed(() => {
  const raw = props.account.serverUrl?.trim()
  if (!raw)
    return ''
  try {
    return new URL(raw.includes('://') ? raw : `http://${raw}`).host
  }
  catch {
    return raw.replace(/^https?:\/\//, '').split('/')[0] ?? raw
  }
})

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
  <section class="flex flex-col gap-4 rounded-2xl bg-surface-container-high/60 p-4 ring-1 ring-white/6">
    <div class="flex min-w-0 items-center gap-3">
      <v-icon :icon="mdiAccountCircle" size="32" class="shrink-0 text-primary" />
      <div class="min-w-0">
        <p class="truncate text-title-small font-semibold">
          {{ label }}
        </p>
        <p class="truncate text-label-small opacity-55">
          {{ kind }}
          <template v-if="account.isTrial">
            · {{ $t('Trial') }}
          </template>
        </p>
      </div>
    </div>

    <dl class="grid gap-2 text-body-small sm:grid-cols-2">
      <div v-if="serverHost" class="rounded-xl bg-surface-container px-3 py-2">
        <dt class="text-label-small opacity-45">
          {{ $t('Server') }}
        </dt>
        <dd class="truncate font-medium">
          {{ serverHost }}
        </dd>
      </div>
      <div v-if="account.username" class="rounded-xl bg-surface-container px-3 py-2">
        <dt class="text-label-small opacity-45">
          {{ $t('Username') }}
        </dt>
        <dd class="truncate font-medium">
          {{ account.username }}
        </dd>
      </div>
      <div v-if="expiry" class="rounded-xl bg-surface-container px-3 py-2">
        <dt class="text-label-small opacity-45">
          {{ $t('Expires') }}
        </dt>
        <dd class="font-medium">
          {{ expiry }}
        </dd>
      </div>
      <div v-if="connections" class="rounded-xl bg-surface-container px-3 py-2">
        <dt class="text-label-small opacity-45">
          {{ $t('Connections') }}
        </dt>
        <dd class="font-medium tabular-nums">
          {{ connections }}
        </dd>
      </div>
      <div class="rounded-xl bg-surface-container px-3 py-2">
        <dt class="text-label-small opacity-45">
          {{ $t('Status') }}
        </dt>
        <dd class="font-medium capitalize">
          {{ account.status || $t('Connected') }}
        </dd>
      </div>
    </dl>

    <dl v-if="catalog" class="flex flex-wrap items-center gap-x-5 gap-y-1 border-t border-outline/15 pt-3 text-label-small">
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
      <div v-if="epgAge" class="flex items-baseline gap-1.5">
        <dt class="opacity-45">
          {{ $t('Guide') }}
        </dt>
        <dd class="opacity-75">
          {{ epgAge }}
        </dd>
      </div>
    </dl>

    <div class="flex flex-wrap items-center gap-2 border-t border-outline/15 pt-3">
      <button
        type="button"
        class="inline-flex min-h-10 items-center gap-2 rounded-xl bg-surface-container px-3 text-body-small font-medium transition-colors hover:bg-surface-container-highest focus-visible:bg-surface-container-highest focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-40"
        :disabled="busy || syncing"
        @click="emit('refresh')"
      >
        <v-icon :icon="mdiRefresh" size="18" :class="(busy || syncing) ? 'animate-spin' : undefined" />
        {{ $t('Refresh') }}
      </button>
      <button
        type="button"
        class="inline-flex min-h-10 items-center gap-2 rounded-xl bg-error/10 px-3 text-body-small font-medium text-error transition-colors hover:bg-error/20 focus-visible:bg-error/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-error disabled:opacity-40"
        :disabled="busy"
        @click="emit('disconnect')"
      >
        <v-icon :icon="mdiLogout" size="18" />
        {{ $t('Disconnect') }}
      </button>
    </div>
  </section>
</template>
