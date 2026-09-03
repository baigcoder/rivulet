<script setup lang="ts">
import { mdiArrowRight, mdiCastConnected, mdiTelevisionClassic } from '@mdi/js'

definePageMeta({ layout: 'default' })

const settings = useSettingsStore()
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

    <div class="mt-5 grid min-h-0 flex-1 auto-rows-fr grid-cols-1 gap-4 sm:grid-cols-2">
      <nuxt-link
        :to="localePath('/live-tv/free')"
        class="group relative flex h-full min-h-56 flex-col justify-center gap-8 overflow-hidden rounded-3xl border border-outline/20 bg-surface-container-high/50 p-8 transition-colors hover:border-primary/40 focus-visible:border-primary/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary md:p-10 lg:gap-10 lg:p-12"
      >
        <v-icon
          :icon="mdiTelevisionClassic"
          size="220"
          class="pointer-events-none absolute -bottom-10 -end-8 opacity-[0.07]"
        />
        <div class="relative flex items-start gap-5 lg:gap-6">
          <div class="grid size-20 shrink-0 place-items-center rounded-3xl bg-primary/15 text-primary transition-colors group-hover:bg-primary group-hover:text-on-primary group-focus-visible:bg-primary group-focus-visible:text-on-primary lg:size-24">
            <v-icon :icon="mdiTelevisionClassic" size="44" />
          </div>
          <div class="min-w-0 flex-1">
            <h2 class="text-headline-large font-bold tracking-tight lg:text-display-small">
              {{ $t('Free TV') }}
            </h2>
            <p class="mt-3 max-w-md text-title-medium opacity-60 lg:text-title-large">
              {{ $t('Public live television from around the world.') }}
            </p>
          </div>
          <v-icon :icon="mdiArrowRight" size="36" class="mt-2 shrink-0 opacity-40 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
        </div>
        <ul class="relative flex flex-wrap gap-3 text-title-medium opacity-80">
          <li class="rounded-full bg-surface-container px-5 py-2.5">
            {{ $t('No account needed') }}
          </li>
          <li class="rounded-full bg-surface-container px-5 py-2.5">
            {{ $t('TV guide') }}
          </li>
          <li class="rounded-full bg-surface-container px-5 py-2.5">
            {{ $t('Favourites') }}
          </li>
        </ul>
      </nuxt-link>

      <nuxt-link
        :to="localePath('/live-tv/premium')"
        class="group relative flex h-full min-h-56 flex-col justify-center gap-8 overflow-hidden rounded-3xl border border-outline/20 bg-surface-container-high/50 p-8 transition-colors hover:border-primary/40 focus-visible:border-primary/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary md:p-10 lg:gap-10 lg:p-12"
      >
        <v-icon
          :icon="mdiCastConnected"
          size="220"
          class="pointer-events-none absolute -bottom-10 -end-8 opacity-[0.07]"
        />
        <div class="relative flex items-start gap-5 lg:gap-6">
          <div class="grid size-20 shrink-0 place-items-center rounded-3xl bg-primary/15 text-primary transition-colors group-hover:bg-primary group-hover:text-on-primary group-focus-visible:bg-primary group-focus-visible:text-on-primary lg:size-24">
            <v-icon :icon="mdiCastConnected" size="44" />
          </div>
          <div class="min-w-0 flex-1">
            <h2 class="text-headline-large font-bold tracking-tight lg:text-display-small">
              {{ $t('Premium TV') }}
            </h2>
            <p class="mt-3 max-w-md text-title-medium opacity-60 lg:text-title-large">
              {{ settings.isPremium ? $t('Your IPTV provider — live, movies and shows.') : $t('Requires subscription') }}
            </p>
          </div>
          <v-icon :icon="mdiArrowRight" size="36" class="mt-2 shrink-0 opacity-40 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
        </div>
        <ul class="relative flex flex-wrap gap-3 text-title-medium opacity-80">
          <li class="rounded-full bg-surface-container px-5 py-2.5">
            {{ $t('Xtream & M3U') }}
          </li>
          <li class="rounded-full bg-surface-container px-5 py-2.5">
            {{ $t('Movies & series') }}
          </li>
        </ul>
      </nuxt-link>
    </div>
  </div>
</template>
