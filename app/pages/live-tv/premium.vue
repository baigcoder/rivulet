<script setup lang="ts">
/**
 * Premium TV's route shell.
 *
 * Two states, and only ever one of them on screen: the connect form (no
 * paid provider signed in yet) or the browser. There used to be a third,
 * "locked", behind a local subscription switch — Premium TV is now whatever
 * the user's own paid provider account gives them. Nested routes replace
 * both rather than rendering under them — `watch` is a full-screen player and `category` is its own
 * heading, and the previous build rendered both *below* the dashboard
 * because the nested check was a regex that never matched.
 */
import { computed, onMounted, ref } from 'vue'

definePageMeta({ layout: 'default' })

const premium = usePremiumTvStore()
const route = useRoute()
const statusLoaded = ref(false)

/**
 * No trailing slash. With one, `/live-tv/premium/watch` did not match and
 * the shell drew the browser over the player.
 */
const isNestedRoute = computed(() =>
  /\/live-tv\/premium\/(?:watch|connect|category|series|movie)/.test(route.path),
)

async function loadInitialStatus(): Promise<void> {
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
</script>

<template>
  <nuxt-page v-if="isNestedRoute" />

  <div v-else class="flex h-full min-h-0 flex-col gap-3 px-4 py-4 md:px-6">
    <premium-tv-premium-browser v-if="premium.connected" />

    <!-- Only the initial status lookup owns this spinner. During Connect the
         form stays mounted so progress and errors remain visible. -->
    <div v-else-if="!statusLoaded" class="grid flex-1 place-items-center">
      <v-progress-circular indeterminate color="primary" size="36" />
    </div>

    <div v-else class="mx-auto flex w-full max-w-3xl flex-col gap-4">
      <v-alert
        v-if="premium.expired"
        type="error"
        variant="tonal"
        :text="$t('Your subscription has expired. Please renew with your provider.')"
      >
        <template #append>
          <v-btn variant="text" size="small" :to="localePath('/settings/premium-tv')">
            {{ $t('Account') }}
          </v-btn>
        </template>
      </v-alert>
      <premium-tv-premium-connect-form />
    </div>
  </div>
</template>
