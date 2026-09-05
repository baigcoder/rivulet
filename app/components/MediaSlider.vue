<script setup lang="ts">
import type { FeedRequest } from '~/composables/useMediaFeed'
import { mdiAlertCircle, mdiArrowRight } from '@mdi/js'

const props = defineProps<{ title: string, request: FeedRequest, to?: string }>()

const ui = useUiStore()

const { items, pending, error } = useMediaFeed(() => props.request)

/** Cap at 20 items — roughly 3 viewport widths of posters. */
const MAX_SLIDER_ITEMS = 20
const visible = computed(() => items.value.slice(0, MAX_SLIDER_ITEMS))
</script>

<template>
  <scroll-row :title="title" :to="to">
    <template v-if="error && !items.length">
      <div class="flex items-center gap-2 px-4 py-3 text-body-small text-error/80">
        <v-icon :icon="mdiAlertCircle" size="16" />
        {{ $t('Couldn\'t load this title.') }}
      </div>
    </template>
    <template v-else>
      <media-card
        v-for="media in visible"
        :key="`${media.type}-${media.id}`"
        :media="media"
        :detail="ui.isDetailed"
        class="shrink-0"
        :style="{ width: `${ui.cardWidth}px` }"
      />
      <div
        v-for="n in pending && !items.length ? 8 : 0"
        :key="`skeleton-${n}`"
        class="animate-pulse aspect-2/3 shrink-0 rounded-xl bg-surface-container/60"
        :style="{ width: `${ui.cardWidth}px` }"
      />
      <!-- Show More card. Plain <a> with :href (for right-click) AND @click
           (as a fallback if navigation is flaky). -->
      <a
        v-if="to"
        :href="localePath(to)"
        class="flex shrink-0 cursor-pointer flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed border-outline-variant/40 bg-surface-container/30 transition-colors duration-200 hover:border-primary/60 hover:bg-surface-container/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        :style="{ width: `${ui.cardWidth}px`, minHeight: `${Math.round(ui.cardWidth * 1.5) + 40}px` }"
        @click.stop.prevent="navigateTo(localePath(to))"
      >
        <v-icon :icon="mdiArrowRight" size="32" class="text-primary opacity-70" />
        <span class="text-label-medium font-medium text-primary opacity-80">{{ $t('See all {title}', { title }) }}</span>
      </a>
    </template>
  </scroll-row>
</template>
