<script setup lang="ts">
/**
 * Premium TV's route shell.
 *
 * Three states, and only ever one of them on screen: locked (no premium
 * tier on this install), the connect form (premium, no provider), or the
 * browser. Nested routes replace all three rather than rendering under
 * them — `watch` is a full-screen player and `category` is its own
 * heading, and the previous build rendered both *below* the dashboard
 * because the nested check was a regex that never matched.
 */
import { mdiArrowRight, mdiCastConnected, mdiMovieOpenOutline, mdiStarOutline, mdiTelevisionGuide } from '@mdi/js'
import { computed, onMounted, ref, watch } from 'vue'

definePageMeta({ layout: 'default' })

const settings = useSettingsStore()
const premium = usePremiumTvStore()
const route = useRoute()
const statusLoaded = ref(false)

const isLocked = computed(() => !settings.isPremium)

const features = computed(() => [
  { icon: mdiTelevisionGuide, title: $t('Live with a guide'), body: $t('What is on now and next, by category.') },
  { icon: mdiMovieOpenOutline, title: $t('Movies & series'), body: $t('Your provider\'s catalogue, with posters and seasons.') },
  { icon: mdiStarOutline, title: $t('Favourites'), body: $t('Star channels and find them again in one press.') },
])

/**
 * No trailing slash. With one, `/live-tv/premium/watch` did not match and
 * the shell drew the browser over the player.
 */
const isNestedRoute = computed(() =>
  /\/live-tv\/premium\/(?:watch|connect|category|series|movie)/.test(route.path),
)

async function loadInitialStatus(): Promise<void> {
  if (settings.isPremium)
    await premium.loadStatus()
  statusLoaded.value = true
  // Movies / TV shows wait on the panel. Start that while the live
  // grid is still painting, so the tab is warm when they open it.
  if (premium.connected && premium.supportsVod)
    void premium.prefetchVod()
}

onMounted(() => {
  void loadInitialStatus()
})

watch(() => settings.isPremium, active => {
  if (active && statusLoaded.value)
    void premium.loadStatus()
})
</script>

<template>
  <nuxt-page v-if="isNestedRoute" />

  <div v-else class="flex h-full min-h-0 flex-col gap-3 px-4 py-4 md:px-6">
    <!-- No premium tier: the API would refuse every call, so nothing is
         fetched and the panel that fixes it is one button away. -->
    <!-- Not set up yet: say what it is and what it brings, not only that it
         is locked — this is the screen someone sees before deciding. -->
    <div
      v-if="isLocked"
      class="relative mx-auto my-auto flex w-full max-w-3xl flex-col gap-6 overflow-hidden rounded-3xl border border-white/10 bg-surface-container-high p-8 md:p-10"
    >
      <div class="pointer-events-none absolute inset-0 bg-gradient-to-br from-tertiary/25 via-transparent to-transparent" />
      <v-icon :icon="mdiCastConnected" size="220" class="pointer-events-none absolute -bottom-12 -end-10 opacity-[0.07]" />

      <div class="relative flex flex-col gap-2">
        <span class="self-start rounded-full bg-tertiary/20 px-3 py-1 text-label-large font-semibold text-tertiary">
          {{ $t('Not set up') }}
        </span>
        <h1 class="text-headline-large font-bold tracking-tight">
          {{ $t('Premium TV') }}
        </h1>
        <p class="max-w-xl text-title-medium opacity-80">
          {{ $t('Watch your own IPTV provider here — its live channels with a TV guide, and its movies and series.') }}
        </p>
      </div>

      <ul class="relative m-0 grid list-none gap-3 p-0 sm:grid-cols-3">
        <li v-for="f in features" :key="f.title" class="flex flex-col gap-1.5 rounded-2xl bg-surface-container/70 p-4">
          <v-icon :icon="f.icon" size="24" class="text-tertiary" />
          <span class="text-title-small font-semibold">{{ f.title }}</span>
          <span class="text-body-small opacity-70">{{ f.body }}</span>
        </li>
      </ul>

      <div class="relative flex flex-wrap items-center gap-3">
        <v-btn color="primary" variant="flat" size="large" rounded="lg" :append-icon="mdiArrowRight" :to="localePath('/settings/premium-tv')">
          {{ $t('Set up Premium TV') }}
        </v-btn>
        <span class="text-body-small opacity-60">{{ $t('Xtream Codes or an M3U playlist from your provider.') }}</span>
      </div>
    </div>

    <premium-tv-premium-browser v-else-if="premium.connected" />

    <!-- Only the initial status lookup owns this spinner. During Connect the
         form stays mounted so progress and errors remain visible. -->
    <div v-else-if="!statusLoaded" class="grid flex-1 place-items-center">
      <v-progress-circular indeterminate color="primary" size="36" />
    </div>

    <div v-else class="mx-auto w-full max-w-3xl">
      <premium-tv-premium-connect-form />
    </div>
  </div>
</template>
