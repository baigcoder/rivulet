<script setup lang="ts">
import type { Review } from '~/utils/tmdb'
import { mdiAccountCircle, mdiStar } from '@mdi/js'

/**
 * Two reviews on the page itself, cut to a few lines each. The full list stays
 * behind the rating, where it was; this is the taste of it that tells you
 * whether to open it.
 */
defineProps<{ reviews: Review[] }>()

const expanded = ref<string | null>(null)

function dateText(iso: string) {
  try {
    return new Intl.DateTimeFormat(uiLocale(), { dateStyle: 'medium' }).format(new Date(iso))
  }
  catch {
    return ''
  }
}
</script>

<template>
  <section v-if="reviews.length" class="mx-4 flex flex-col gap-3 md:mx-6">
    <h2 class="text-title-large font-bold">
      {{ $t('Reviews') }}
    </h2>
    <div class="grid gap-4 lg:grid-cols-2">
      <article
        v-for="r in reviews"
        :key="r.id"
        class="flex flex-col gap-3 rounded-2xl border border-white/10 bg-surface-container-high/40 p-5"
      >
        <header class="flex items-center gap-3">
          <img v-if="r.avatar" :src="r.avatar" alt="" class="size-10 rounded-full object-cover">
          <v-icon v-else :icon="mdiAccountCircle" size="40" class="opacity-50" />
          <div class="min-w-0 flex-1">
            <p class="truncate text-title-small font-semibold">
              {{ r.author }}
            </p>
            <p class="text-body-small opacity-55">
              {{ dateText(r.created) }}
            </p>
          </div>
          <span v-if="r.rating" class="flex items-center gap-1 rounded-full bg-amber-400/15 px-2.5 py-1 text-label-large font-semibold text-amber-300">
            <v-icon :icon="mdiStar" size="16" />{{ r.rating }}/10
          </span>
        </header>
        <p class="whitespace-pre-line text-body-medium leading-relaxed opacity-85" :class="expanded === r.id ? '' : 'line-clamp-4'">
          {{ r.content }}
        </p>
        <button
          v-if="r.content.length > 320"
          type="button"
          class="self-start rounded-full px-3 py-1 text-label-large font-medium text-primary outline-none hover:bg-primary/10 focus-visible:bg-primary/10 focus-visible:ring-2 focus-visible:ring-primary"
          @click="expanded = expanded === r.id ? null : r.id"
        >
          {{ expanded === r.id ? $t('Show less') : $t('Read more') }}
        </button>
      </article>
    </div>
  </section>
</template>
