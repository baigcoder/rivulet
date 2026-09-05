<script setup lang="ts">
import type { MediaType } from '~/utils/tmdb'

// Keeps /foo/123 out of here; anything else 404s instead of asking TMDB.
definePageMeta({
  validate: ({ params }) => 'type' in params && (params.type === 'movie' || params.type === 'tv'),
})

const route = useRoute()
const type = computed(() => route.params.type as MediaType)
const id = computed(() => String(route.params.id))
</script>

<template>
  <media-detail-view :id="id" :type="type" />
</template>
