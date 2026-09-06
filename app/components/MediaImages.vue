<script setup lang="ts">
/**
 * Backdrop stills for a title. Fetched when this row mounts — never with
 * the first title request, which is why /images stays off DETAIL_CORE.
 */
import type { MediaType } from '~/utils/tmdb'
import { mdiMagnifyPlus } from '@mdi/js'
import { useTitleImages } from '~/utils/titleImages'

const props = defineProps<{ type: MediaType, id: string, stills?: string[] }>()

const { data: art, status, execute } = useTitleImages(() => props.type, () => props.id)
const stills = computed(() => props.stills?.length ? props.stills : art.value.stills)
onMounted(() => {
  if (props.id && !props.stills?.length)
    void execute()
})

const current = ref<string | null>(null)
</script>

<template>
  <scroll-row
    v-if="stills.length || status === 'pending'"
    :title="$t('Images')"
    :count="stills.length || undefined"
  >
    <button
      v-for="(path, i) in stills"
      :key="path"
      type="button"
      class="group relative w-56 shrink-0 overflow-hidden rounded-2xl bg-surface-container shadow-md ring-1 ring-white/10 transition-all duration-300 hover:scale-[1.03] hover:shadow-xl outline-none sm:w-64"
      :aria-label="$t('Images')"
      @click="current = path"
    >
      <div class="aspect-video relative overflow-hidden rounded-2xl">
        <media-poster :src="stillUrl(path, 'w300')" :alt="`${$t('Images')} ${i + 1}`" />
        <div class="grid place-items-center absolute inset-0 bg-black/45 opacity-0 transition-opacity duration-200 group-hover:opacity-100 group-focus-visible:opacity-100">
          <v-icon :icon="mdiMagnifyPlus" size="28" class="text-white drop-shadow-md" />
        </div>
      </div>
      <div class="pointer-events-none absolute inset-0 rounded-2xl opacity-0 ring-2 ring-inset ring-primary transition-opacity duration-200 group-hover:opacity-100 group-focus-visible:opacity-100" />
    </button>
    <div
      v-for="n in status === 'pending' && !stills.length ? 6 : 0"
      :key="`image-skeleton-${n}`"
      class="aspect-video w-56 shrink-0 animate-pulse rounded-2xl bg-surface-container sm:w-64"
    />
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
