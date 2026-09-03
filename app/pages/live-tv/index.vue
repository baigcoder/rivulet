<script setup lang="ts">
import { mdiArrowRight, mdiCastConnected, mdiTelevisionClassic } from '@mdi/js'

definePageMeta({ layout: 'default' })

const settings = useSettingsStore()

const tiles = computed(() => [
  {
    to: localePath('/live-tv/free'),
    icon: mdiTelevisionClassic,
    title: $t('Free TV'),
    body: $t('Public live television from around the world.'),
    tags: [$t('No account needed'), $t('TV guide'), $t('Favourites')],
  },
  {
    to: localePath('/live-tv/premium'),
    icon: mdiCastConnected,
    title: $t('Premium TV'),
    body: settings.isPremium
      ? $t('Your IPTV provider — live, movies and shows.')
      : $t('Requires subscription'),
    tags: [$t('Xtream & M3U'), $t('Movies & series')],
  },
])
</script>

<template>
  <div class="flex h-full flex-col px-4 py-5 md:px-6">
    <header class="shrink-0">
      <h1 class="text-headline-small font-bold tracking-tight">
        {{ $t('Live TV') }}
      </h1>
      <p class="mt-1 text-body-medium opacity-60">
        {{ $t('Browse free channels or sign in to your IPTV provider.') }}
      </p>
    </header>

    <div class="mt-5 grid min-h-0 flex-1 auto-rows-fr grid-cols-1 gap-3 sm:grid-cols-2 sm:gap-4">
      <nuxt-link
        v-for="tile in tiles"
        :key="tile.to"
        :to="tile.to"
        class="group relative flex h-full min-h-44 flex-col justify-center gap-5 overflow-hidden rounded-2xl border border-outline/20 bg-surface-container-high p-5 transition-colors hover:border-primary/40 focus-visible:border-primary/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary sm:min-h-56 sm:gap-8 sm:p-8 lg:gap-10 lg:p-12"
      >
        <v-icon
          :icon="tile.icon"
          size="160"
          class="pointer-events-none absolute -bottom-8 -end-6 opacity-[0.07] sm:size-[220px]"
        />
        <div class="relative flex items-center gap-3 sm:gap-5 lg:gap-6">
          <div class="grid size-12 shrink-0 place-items-center rounded-xl bg-primary/15 text-primary transition-colors group-hover:bg-primary group-hover:text-on-primary group-focus-visible:bg-primary group-focus-visible:text-on-primary sm:size-16 sm:rounded-2xl lg:size-20 lg:rounded-3xl">
            <v-icon :icon="tile.icon" size="28" />
          </div>
          <div class="min-w-0 flex-1">
            <h2 class="flex items-center gap-2 text-title-large font-bold tracking-tight sm:text-headline-small lg:text-headline-large">
              <span class="truncate">{{ tile.title }}</span>
              <v-icon :icon="mdiArrowRight" size="20" class="shrink-0 opacity-40 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100 sm:size-7" />
            </h2>
            <p class="mt-1 max-w-md text-body-medium opacity-60 sm:mt-2 sm:text-title-medium lg:text-title-large">
              {{ tile.body }}
            </p>
          </div>
        </div>
        <ul class="relative flex list-none flex-wrap gap-2 ps-0 text-label-large opacity-80 sm:gap-3 sm:text-title-medium">
          <li
            v-for="tag in tile.tags"
            :key="tag"
            class="rounded-full bg-surface-container px-3 py-1.5 sm:px-5 sm:py-2.5"
          >
            {{ tag }}
          </li>
        </ul>
      </nuxt-link>
    </div>
  </div>
</template>
