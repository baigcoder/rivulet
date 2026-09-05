<script setup lang="ts">
import type { MediaType } from '~/utils/tmdb'
import { mdiStar } from '@mdi/js'

const props = defineProps<{
  type: MediaType
  id: string
  rating: number
  votes?: number
}>()

const open = ref(false)
const sort = ref<'popular' | 'new'>('popular')

const { data: reviews, status, execute } = useReviews(() => props.type, () => props.id)

/** Idle reads as empty — wait until the fetch finishes before saying there are none. */
const waiting = computed(() => open.value && status.value !== 'success' && status.value !== 'error')

watch(open, value => {
  if (value)
    void execute()
})

onMounted(() => {
  if (import.meta.server)
    return
  const warm = () => void execute()
  if ('requestIdleCallback' in globalThis)
    requestIdleCallback(warm)
  else
    setTimeout(warm, 0)
})

const shown = computed(() => {
  const list = [...(reviews.value ?? [])]
  if (sort.value === 'new')
    return list.sort((a, b) => b.created.localeCompare(a.created))
  return list.sort((a, b) => (b.rating ?? -1) - (a.rating ?? -1) || b.created.localeCompare(a.created))
})

function dated(iso: string) {
  const at = Date.parse(iso)
  return Number.isNaN(at) ? '' : new Date(at).toLocaleDateString(uiLocale())
}
</script>

<template>
  <v-menu
    v-model="open"
    :close-on-content-click="false"
    location="bottom start"
    max-width="420"
  >
    <template #activator="{ props: menu }">
      <button
        v-bind="menu"
        type="button"
        class="flex items-center gap-1 rounded-md px-1.5 py-0.5 -mx-1.5 opacity-100 hover:bg-on-surface/10 focus-visible:bg-on-surface/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        :aria-label="$t('Reviews')"
      >
        <v-icon :icon="mdiStar" size="14" class="text-amber-400" />
        <span class="font-medium">{{ rating.toFixed(1) }}</span>
        <span v-if="votes != null" class="opacity-60">({{ votes.toLocaleString(uiLocale()) }})</span>
      </button>
    </template>

    <v-card rounded="xl" class="flex w-[min(calc(100vw-2rem),24rem)] flex-col">
      <div class="flex items-center gap-2 px-3 pt-3">
        <span class="text-title-small font-medium">{{ $t('Reviews') }}</span>
        <v-spacer />
        <button
          type="button"
          class="rounded-full px-3 py-1 text-label-medium hover:bg-on-surface/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          :class="sort === 'popular' ? 'bg-primary text-on-primary' : 'opacity-70'"
          @click="sort = 'popular'"
        >
          {{ $t('Popular') }}
        </button>
        <button
          type="button"
          class="rounded-full px-3 py-1 text-label-medium hover:bg-on-surface/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          :class="sort === 'new' ? 'bg-primary text-on-primary' : 'opacity-70'"
          @click="sort = 'new'"
        >
          {{ $t('New') }}
        </button>
      </div>

      <div class="max-h-[min(70vh,28rem)] overflow-y-auto px-3 py-2">
        <div v-if="waiting" class="flex flex-col gap-3 py-2">
          <div v-for="n in 3" :key="n" class="animate-pulse h-16 rounded-lg bg-surface-container/60" />
        </div>
        <p v-else-if="status === 'error'" class="py-8 text-center text-body-small opacity-70">
          {{ $t('Couldn\'t load reviews.') }}
        </p>
        <p v-else-if="!shown.length" class="py-8 text-center text-body-small opacity-70">
          {{ $t('No reviews yet.') }}
        </p>
        <article
          v-for="review in shown"
          :key="review.id"
          class="border-b border-outline-variant/40 py-3 last:border-0"
        >
          <div class="flex items-center gap-2">
            <img
              v-if="review.avatar"
              :src="review.avatar"
              :alt="review.author"
              class="size-8 shrink-0 rounded-full object-cover bg-surface-container"
            >
            <div v-else class="grid size-8 shrink-0 place-items-center rounded-full bg-surface-container text-label-small">
              {{ review.author.slice(0, 1) }}
            </div>
            <div class="min-w-0 flex-1">
              <div class="truncate text-label-large">
                {{ review.author }}
              </div>
              <div class="text-label-small opacity-55">
                {{ dated(review.created) }}
              </div>
            </div>
            <span v-if="review.rating != null" class="flex shrink-0 items-center gap-0.5 text-label-medium">
              <v-icon :icon="mdiStar" size="12" class="text-amber-400" />
              {{ review.rating }}
            </span>
          </div>
          <p class="mt-2 whitespace-pre-wrap text-body-small opacity-85">
            {{ review.content }}
          </p>
        </article>
      </div>
    </v-card>
  </v-menu>
</template>
