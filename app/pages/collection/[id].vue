<script setup lang="ts">
import { mdiAlertCircleOutline, mdiArrowLeft } from '@mdi/js'

definePageMeta({
  validate: ({ params }) => /^\d+$/.test(String((params as Record<string, string>).id)),
})

const route = useRoute()
const ui = useUiStore()

const id = computed(() => String((route.params as Record<string, string>).id))

const { data: collection, pending, error } = useCollectionDetail(id)

let mine = 0
watch(collection, value => {
  if (value) {
    mine = ui.select({
      id: value.id,
      type: 'movie',
      title: value.name,
      year: '',
      poster: value.poster,
      backdrop: value.backdrop,
      overview: value.overview,
      rating: 0,
      genreIds: [],
    })
  }
}, { immediate: true })
onUnmounted(() => ui.release(mine))
</script>

<template>
  <div class="h-full overflow-y-auto pb-12">
    <div v-if="error" class="flex h-full flex-col items-center justify-center gap-2">
      <v-icon :icon="mdiAlertCircleOutline" color="error" size="40" />
      <span class="text-body-medium opacity-70">{{ $t('Couldn\'t load this collection.') }}</span>
      <v-btn variant="tonal" :to="localePath('/')">
        {{ $t('Back to home') }}
      </v-btn>
    </div>

    <template v-else>
      <section class="px-4 pb-8 pt-4 md:px-6">
        <v-btn :prepend-icon="mdiArrowLeft" variant="text" size="small" class="mb-3 -ml-2" @click="$router.back()">
          {{ $t('Back') }}
        </v-btn>

        <div class="flex flex-col gap-6 sm:flex-row sm:items-end">
          <div v-if="collection?.poster" class="aspect-2/3 w-32 shrink-0 overflow-hidden rounded-2xl shadow-2xl sm:w-40">
            <media-poster :src="posterUrl(collection.poster, 'w342')" :alt="collection?.name" />
          </div>

          <div class="flex min-w-0 flex-1 flex-col gap-3">
            <h1 class="text-headline-large font-bold drop-shadow-[0_2px_24px_rgba(0,0,0,0.6)]">
              {{ collection?.name ?? $t('Loading…') }}
            </h1>

            <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-body-small opacity-75">
              <span v-if="collection?.parts.length">{{ $t('{count} films', { count: collection.parts.length }) }}</span>
            </div>

            <p v-if="collection?.overview" class="max-w-3xl text-body-medium opacity-85">
              {{ collection.overview }}
            </p>
          </div>
        </div>
      </section>

      <section v-if="collection?.parts.length" class="px-4 md:px-6">
        <media-layout :items="collection.parts" :pending="pending" :done="true" />
      </section>
    </template>
  </div>
</template>
