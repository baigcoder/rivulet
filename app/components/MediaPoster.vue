<script setup lang="ts">
import { mdiImageOffOutline } from '@mdi/js'

// Plain <img loading="lazy"> instead of <v-img>: the grid renders hundreds of
// these and v-img adds an intersection observer plus a transition per instance,
// which is what made switching layouts stutter. The browser does it for free.
const props = defineProps<{
  /** Already-sized TMDB url, or null for the placeholder. */
  src?: string | null
  alt?: string
  /** Fills its parent, which owns the aspect ratio and rounding. */
  icon?: string
  /** Title hero / side poster — already on screen from the card. */
  eager?: boolean
}>()

// Shimmer until the picture is in, then fade it in — see `.poster-shimmer` in
// layers.css. A picture already in the cache can finish before the listener is
// attached, so mounting checks `complete` as well.
const img = ref<HTMLImageElement | null>(null)
const loaded = ref(false)
watch(() => props.src, () => (loaded.value = false))
onMounted(() => {
  if (img.value?.complete && img.value.naturalWidth)
    loaded.value = true
})
</script>

<template>
  <div class="relative size-full overflow-hidden bg-surface-container-high">
    <div v-if="src && !loaded" class="poster-shimmer" aria-hidden="true" />
    <img
      v-if="src"
      ref="img"
      :src="src"
      :alt="alt"
      :loading="eager ? 'eager' : 'lazy'"
      :fetchpriority="eager ? 'high' : undefined"
      decoding="async"
      draggable="false"
      class="size-full object-cover"
      :class="loaded ? 'motion-fade' : 'opacity-0'"
      @load="loaded = true"
      @error="loaded = true"
    >
    <div v-else class="grid size-full place-items-center bg-gradient-to-br from-surface-container-high to-surface-container-lowest">
      <v-icon :icon="icon ?? mdiImageOffOutline" size="28" class="opacity-25" />
    </div>
  </div>
</template>
