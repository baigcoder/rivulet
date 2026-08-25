<script setup lang="ts">
import { mdiBookmarkOutline, mdiHeartOutline, mdiHistory, mdiTrayArrowDown } from '@mdi/js'

const tabs = computed(() => [
  { title: $t('Watchlist'), icon: mdiBookmarkOutline, to: localePath('/library/watchlist') },
  { title: $t('Favourites'), icon: mdiHeartOutline, to: localePath('/library/favourites') },
  { title: $t('History'), icon: mdiHistory, to: localePath('/library/history') },
  { title: $t('Downloads'), icon: mdiTrayArrowDown, to: localePath('/library/downloads') },
])
</script>

<template>
  <div class="flex h-full flex-col">
    <div class="shrink-0 border-b border-[rgb(var(--v-theme-outline-variant))] px-4 pt-3 pb-0 sm:px-6">
      <h1 class="text-title-large font-bold tracking-tight mb-2">
        {{ $t('Library') }}
      </h1>
      <v-tabs color="primary" density="compact" class="border-none">
        <v-tab
          v-for="tab in tabs"
          :key="tab.to"
          :to="tab.to"
          :prepend-icon="tab.icon"
          rounded="md"
          class="text-label-large font-medium normal-case tracking-normal px-4"
        >
          {{ tab.title }}
        </v-tab>
      </v-tabs>
    </div>
    <div class="flex-1 min-h-0 overflow-y-auto">
      <nuxt-page />
    </div>
  </div>
</template>
