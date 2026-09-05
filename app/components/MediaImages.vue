<script setup lang="ts">
/**
 * Backdrop stills for a title. Fetched when this row mounts — never with
 * the first title request, which is why /images stays off DETAIL_CORE.
 */
import type { MediaType } from '~/utils/tmdb'
import { useTitleImages } from '~/utils/titleImages'

const props = defineProps<{ type: MediaType, id: string }>()

const { data: stills, execute } = useTitleImages(() => props.type, () => props.id)
onMounted(() => {
  if (props.id)
    void execute()
})

const current = ref<string | null>(null)
</script>

<template>
  <scroll-row v-if="stills?.length" :title="$t('Images')" :count="stills.length">
    <button
      v-for="(path, i) in stills"
      :key="path"
      type="button"
      class="group relative w-52 shrink-0 overflow-hidden rounded-xl bg-surface-container outline-none sm:w-64"
      :aria-label="$t('Images')"
      @click="current = path"
    >
      <div class="aspect-video">
        <media-poster :src="stillUrl(path, 'w300')" :alt="`${$t('Images')} ${i + 1}`" />
      </div>
      <div class="pointer-events-none absolute inset-0 rounded-xl opacity-0 ring-2 ring-inset ring-primary transition-opacity duration-200 group-hover:opacity-100 group-focus-visible:opacity-100" />
    </button>
  </scroll-row>

  <v-dialog v-if="current" :model-value="true" max-width="1100" @update:model-value="v => !v && (current = null)">
    <v-card class="overflow-hidden">
      <img
        v-if="current"
        :src="backdropUrl(current, 'w1280')!"
        :alt="$t('Images')"
        class="aspect-video w-full bg-black object-contain"
      >
      <v-card-actions>
        <v-spacer />
        <v-btn size="small" variant="text" @click="current = null">
          {{ $t('Close') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>
