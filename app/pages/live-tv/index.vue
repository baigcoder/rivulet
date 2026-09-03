<script setup lang="ts">
import { mdiArrowRight, mdiCastConnected, mdiTelevisionClassic, mdiTelevisionGuide } from '@mdi/js'

definePageMeta({ layout: 'default' })

const settings = useSettingsStore()
</script>

<template>
  <div class="mx-4 mt-6 md:mx-6">
    <header class="max-w-2xl">
      <h1 class="text-headline-small font-bold tracking-tight">
        {{ $t('Live TV') }}
      </h1>
      <p class="mt-1 text-body-medium opacity-60">
        {{ $t('Browse free channels or sign in to your IPTV provider.') }}
      </p>
    </header>

    <div class="mt-8 grid max-w-4xl gap-4 sm:grid-cols-2">
      <nuxt-link
        :to="localePath('/live-tv/free')"
        class="group flex flex-col gap-4 rounded-3xl border border-outline/20 bg-surface-container-high/40 p-5 transition-[border-color,transform] duration-200 hover:border-primary/40 hover:-translate-y-0.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
      >
        <div class="flex items-start gap-4">
          <div class="grid size-14 shrink-0 place-items-center rounded-2xl bg-primary/15 text-primary transition-colors group-hover:bg-primary group-hover:text-on-primary group-focus-visible:bg-primary group-focus-visible:text-on-primary">
            <v-icon :icon="mdiTelevisionClassic" size="28" />
          </div>
          <div class="min-w-0 flex-1 pt-0.5">
            <h2 class="text-title-large font-bold">
              {{ $t('Free TV') }}
            </h2>
            <p class="mt-1 text-body-small opacity-60">
              {{ $t('Public live television from around the world.') }}
            </p>
          </div>
          <v-icon :icon="mdiArrowRight" size="22" class="mt-1 shrink-0 opacity-35 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
        </div>
        <ul class="flex flex-wrap gap-2 text-label-small opacity-70">
          <li class="rounded-full bg-surface-container px-2.5 py-1">
            {{ $t('No account needed') }}
          </li>
          <li class="rounded-full bg-surface-container px-2.5 py-1">
            {{ $t('TV guide') }}
          </li>
        </ul>
      </nuxt-link>

      <nuxt-link
        :to="localePath('/live-tv/premium')"
        class="group flex flex-col gap-4 rounded-3xl border border-outline/20 bg-surface-container-high/40 p-5 transition-[border-color,transform] duration-200 hover:border-primary/40 hover:-translate-y-0.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
      >
        <div class="flex items-start gap-4">
          <div class="grid size-14 shrink-0 place-items-center rounded-2xl bg-primary/15 text-primary transition-colors group-hover:bg-primary group-hover:text-on-primary group-focus-visible:bg-primary group-focus-visible:text-on-primary">
            <v-icon :icon="mdiCastConnected" size="28" />
          </div>
          <div class="min-w-0 flex-1 pt-0.5">
            <h2 class="text-title-large font-bold">
              {{ $t('Premium TV') }}
            </h2>
            <p class="mt-1 text-body-small opacity-60">
              {{ settings.isPremium ? $t('Your IPTV provider — live, movies and shows.') : $t('Requires subscription') }}
            </p>
          </div>
          <v-icon :icon="mdiArrowRight" size="22" class="mt-1 shrink-0 opacity-35 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
        </div>
        <ul class="flex flex-wrap gap-2 text-label-small opacity-70">
          <li class="rounded-full bg-surface-container px-2.5 py-1">
            {{ $t('Xtream & M3U') }}
          </li>
          <li class="rounded-full bg-surface-container px-2.5 py-1">
            {{ $t('Movies & series') }}
          </li>
        </ul>
      </nuxt-link>
    </div>

    <nuxt-link
      :to="localePath('/live-tv/free/guide')"
      class="mt-4 inline-flex max-w-4xl items-center gap-2 rounded-2xl border border-outline/15 bg-surface-container/30 px-4 py-3 text-body-small opacity-80 transition-colors hover:border-primary/30 hover:opacity-100 focus-visible:border-primary/30 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
    >
      <v-icon :icon="mdiTelevisionGuide" size="20" class="shrink-0 text-primary" />
      <span>{{ $t('Open the free TV guide') }}</span>
      <v-icon :icon="mdiArrowRight" size="18" class="ms-auto shrink-0 opacity-50" />
    </nuxt-link>
  </div>
</template>
