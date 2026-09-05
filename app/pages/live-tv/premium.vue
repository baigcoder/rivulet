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
import { mdiLock } from '@mdi/js'
import { computed, onMounted, ref, watch } from 'vue'

definePageMeta({ layout: 'default' })

const settings = useSettingsStore()
const premium = usePremiumTvStore()
const route = useRoute()
const statusLoaded = ref(false)

const isLocked = computed(() => !settings.isPremium)

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
    <div
      v-if="isLocked"
      class="mx-auto flex max-w-md flex-col items-center gap-4 rounded-3xl bg-surface-container p-8 text-center ring-1 ring-white/8"
    >
      <v-icon :icon="mdiLock" size="48" class="opacity-40" />
      <h1 class="text-headline-small font-bold">
        {{ $t('Premium TV is not available') }}
      </h1>
      <p class="max-w-md text-body-medium opacity-70">
        {{ $t('Activate your subscription in Settings → Premium TV to unlock this section.') }}
      </p>
      <v-btn color="primary" variant="tonal" :to="localePath('/settings/premium-tv')">
        {{ $t('Open Settings') }}
      </v-btn>
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
