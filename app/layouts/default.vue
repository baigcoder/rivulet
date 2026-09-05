<script setup lang="ts">
const liveTv = useLiveTvStore()
const route = useRoute()
const ui = useUiStore()

// Immediate feedback on press while the keepalive Home tree moves out.
const titleRoute = computed(() => isTitlePath(route.path))
const opening = computed(() => titleRoute.value ? null : ui.opening)
</script>

<template>
  <v-app>
    <app-background />
    <app-drawer />

    <!-- The window never scrolls: the shell is a fixed-height column and each
         page scrolls its own content region, so the chrome stays put. -->
    <v-main class="relative z-1 h-dvh">
      <!-- The backdrop art is fixed behind this and stays full-bleed; only the
           content is pulled in off the system bars. -->
      <div class="safe-inset flex h-full flex-col">
        <app-bar />
        <!-- data-dpad-start: where a remote picks up focus after a navigation,
             so it lands on the page instead of the toolbar above it. -->
        <div
          data-dpad-start
          class="relative min-h-0 flex-1"
          :class="titleRoute ? 'overflow-hidden' : 'overflow-y-auto'"
        >
          <div
            v-if="opening"
            class="pointer-events-none absolute inset-0 z-30 grid place-items-center bg-background"
            aria-hidden="true"
          >
            <div class="flex items-center gap-5">
              <div class="aspect-2/3 w-28 overflow-hidden rounded-xl bg-surface-container shadow-xl">
                <media-poster
                  eager
                  :src="posterUrl(opening.poster, ui.posterSize)"
                  :alt="opening.title"
                />
              </div>
              <p class="max-w-md text-headline-medium font-bold">
                {{ opening.title }}
              </p>
            </div>
          </div>
          <slot />
        </div>
      </div>
    </v-main>

    <!-- Mini player (floating over everything) -->
    <live-tv-live-mini-player
      :channel="liveTv.miniChannel"
      :stream-url="liveTv.miniStreamUrl"
      @close="liveTv.hideMiniPlayer()"
      @expand="liveTv.expandMiniPlayer()"
    />
  </v-app>
</template>
