<script setup lang="ts">
import { mdiCheckCircle } from '@mdi/js'

/**
 * The rest of a franchise, in release order, with the ones already seen marked
 * — "what do I watch after this" answered on the page that raised it. The
 * title being viewed is in the row too, outlined, so the order reads.
 */
const props = defineProps<{
  id: number
  name: string
  /** The title this page is about. */
  current: number
}>()

const ui = useUiStore()
const library = useLibraryStore()
const { data } = useCollectionDetail(() => props.id)
const parts = computed(() => data.value?.parts ?? [])
</script>

<template>
  <scroll-row
    v-if="parts.length > 1"
    :title="$t('{name} · {count} films', { name, count: parts.length })"
    :to="collectionLink(id)"
  >
    <div
      v-for="(m, i) in parts"
      :key="m.id"
      class="relative shrink-0"
      :style="{ width: `${ui.cardWidth}px` }"
    >
      <media-card
        :media="m"
        detail
        :class="m.id === current ? 'rounded-xl ring-2 ring-[var(--title-accent,rgb(var(--v-theme-primary)))] ring-offset-2 ring-offset-transparent' : ''"
      />
      <span class="pointer-events-none absolute start-2 top-2 grid size-7 place-items-center rounded-full bg-black/75 text-label-medium font-bold text-white">
        {{ i + 1 }}
      </span>
      <v-icon
        v-if="library.isWatched(m)"
        :icon="mdiCheckCircle"
        size="22"
        class="pointer-events-none absolute end-2 top-2 rounded-full bg-black/60 text-emerald-400"
      />
    </div>
  </scroll-row>
</template>
