<script setup lang="ts">
/**
 * Shared header for the free and premium live TV pages. Premium adds
 * the account chip and disconnect button; free shows a refresh button
 * for reloading the M3U sources.
 */
import { mdiCastConnected, mdiLogout, mdiRefresh, mdiTelevision } from '@mdi/js'

defineProps<{
  /** Show the premium account chip + disconnect. */
  premium?: boolean
  connected?: boolean
  accountName?: string | null
  /** Re-fetch the channel list (free or premium). */
  loading?: boolean
}>()

const emit = defineEmits<{
  refresh: []
  logout: []
}>()
</script>

<template>
  <div class="flex items-center gap-3 px-4 pt-5 pb-1 md:px-6">
    <!-- Icon badge -->
    <div class="grid size-9 shrink-0 place-items-center rounded-xl bg-primary/15 ring-1 ring-primary/25">
      <v-icon
        :icon="premium && connected ? mdiCastConnected : mdiTelevision"
        size="20"
        color="primary"
      />
    </div>

    <div class="flex min-w-0 flex-col">
      <h1 class="text-headline-small font-bold leading-tight tracking-tight">
        {{ premium ? $t('Premium TV') : $t('Free TV') }}
      </h1>
      <p class="text-label-small text-white/40">
        {{ premium ? $t('Your personal IPTV') : $t('Free public channels') }}
      </p>
    </div>

    <!-- Premium: account chip when connected -->
    <div
      v-if="premium && connected"
      class="flex items-center gap-1.5 rounded-full border border-green-500/25 bg-green-500/10 px-3 py-1"
    >
      <span class="size-1.5 animate-pulse rounded-full bg-green-400" />
      <span class="text-body-small font-medium text-green-400">{{ accountName || $t('Connected') }}</span>
    </div>

    <v-spacer />

    <!-- Refresh button (both free and premium) -->
    <v-btn
      v-if="!premium || connected"
      size="small"
      variant="tonal"
      :icon="mdiRefresh"
      :loading="loading"
      :aria-label="$t('Refresh')"
      @click="emit('refresh')"
    />

    <!-- Logout (premium only) -->
    <v-btn
      v-if="premium && connected"
      size="small"
      variant="tonal"
      color="error"
      :icon="mdiLogout"
      :aria-label="$t('Logout')"
      @click="emit('logout')"
    />
  </div>
</template>
