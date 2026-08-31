<script setup lang="ts">
import { mdiArrowRight, mdiCastConnected, mdiTelevisionClassic } from '@mdi/js'

definePageMeta({ layout: 'default' })

const settings = useSettingsStore()
</script>

<template>
  <div class="mx-4 mt-6 max-w-5xl md:mx-auto md:px-6">
    <!-- Header Hero -->
    <div class="mb-8 text-center sm:text-start">
      <span class="inline-flex items-center gap-1.5 rounded-full bg-primary/15 px-3 py-1 text-[10px] font-bold uppercase tracking-widest text-primary ring-1 ring-primary/30">
        {{ $t('Live Television Hub') }}
      </span>
      <h1 class="mt-2 text-headline-medium font-black tracking-tight text-white md:text-headline-large">
        {{ $t('Live TV') }}
      </h1>
      <p class="mt-1 text-body-large text-white/60">
        {{ $t('Choose a TV source to browse.') }}
      </p>
    </div>

    <div class="grid gap-6 sm:grid-cols-2">
      <!-- Free TV card -->
      <nuxt-link
        :to="localePath('/live-tv/free')"
        class="group relative overflow-hidden rounded-3xl border border-white/10 bg-gradient-to-b from-surface-container-high via-surface-container to-surface-container-lowest p-7 transition-all duration-300 hover:-translate-y-1 hover:border-primary/60 hover:shadow-2xl hover:shadow-primary/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
      >
        <div class="absolute -right-12 -top-12 size-48 rounded-full bg-primary/15 blur-2xl transition-all group-hover:bg-primary/25 pointer-events-none" />

        <div class="relative z-10 mb-5 flex items-center justify-between">
          <div class="grid size-14 place-items-center rounded-2xl bg-gradient-to-br from-primary/30 to-primary/10 ring-1 ring-primary/30 shadow-lg shadow-primary/20">
            <v-icon :icon="mdiTelevisionClassic" size="30" color="primary" />
          </div>
          <span class="rounded-full bg-white/10 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-white/70">
            {{ $t('Public Streams') }}
          </span>
        </div>

        <div class="relative z-10">
          <h2 class="text-headline-small font-bold text-white">
            {{ $t('Free TV') }}
          </h2>
          <p class="mt-1 text-body-small font-medium text-primary">
            {{ $t('No IPTV account required') }}
          </p>
          <p class="my-4 text-body-medium text-white/65">
            {{ $t('Public live television from around the world.') }}
          </p>

          <div class="inline-flex items-center gap-2 rounded-xl bg-primary/20 px-4 py-2.5 text-body-small font-bold text-primary transition-all group-hover:bg-primary group-hover:text-on-primary">
            <span>{{ $t('Browse Free TV') }}</span>
            <v-icon :icon="mdiArrowRight" size="16" />
          </div>
        </div>
      </nuxt-link>

      <!-- Premium TV card -->
      <nuxt-link
        :to="localePath('/live-tv/premium')"
        class="group relative overflow-hidden rounded-3xl border border-white/10 bg-gradient-to-b from-surface-container-high via-surface-container to-surface-container-lowest p-7 transition-all duration-300 hover:-translate-y-1 hover:border-secondary/60 hover:shadow-2xl hover:shadow-secondary/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-secondary"
      >
        <div class="absolute -right-12 -top-12 size-48 rounded-full bg-secondary/15 blur-2xl transition-all group-hover:bg-secondary/25 pointer-events-none" />

        <div class="relative z-10 mb-5 flex items-center justify-between">
          <div class="grid size-14 place-items-center rounded-2xl bg-gradient-to-br from-secondary/30 to-secondary/10 ring-1 ring-secondary/30 shadow-lg shadow-secondary/20">
            <v-icon :icon="mdiCastConnected" size="30" color="secondary" />
          </div>
          <span class="rounded-full bg-white/10 px-3 py-1 text-[11px] font-bold uppercase tracking-wider text-white/70">
            {{ $t('IPTV Account') }}
          </span>
        </div>

        <div class="relative z-10">
          <h2 class="text-headline-small font-bold text-white">
            {{ $t('Premium TV') }}
          </h2>
          <p class="mt-1 text-body-small font-medium text-secondary">
            {{ settings.isPremium ? $t('Subscription active') : $t('Requires subscription') }}
          </p>
          <p class="my-4 text-body-medium text-white/65">
            {{ $t('Xtream and M3U IPTV sources with EPG and more.') }}
          </p>

          <div class="inline-flex items-center gap-2 rounded-xl bg-secondary/20 px-4 py-2.5 text-body-small font-bold text-secondary transition-all group-hover:bg-secondary group-hover:text-on-secondary">
            <span>{{ $t('Connect IPTV') }}</span>
            <v-icon :icon="mdiArrowRight" size="16" />
          </div>
        </div>
      </nuxt-link>
    </div>
  </div>
</template>
