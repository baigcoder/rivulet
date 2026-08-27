<script setup lang="ts">
const route = useRoute()
const initialCategory = computed(() => String(route.query.cat ?? '') || undefined)
const lang = computed(() => String(route.query.lang ?? '') || undefined)
const label = computed(() => String(route.query.label ?? '') || undefined)

definePageMeta({ keepalive: true })
</script>

<template>
  <media-browser
    :key="route.fullPath"
    type="movie"
    :initial-category="initialCategory"
    :extra-params="{ with_original_language: lang }"
    :page-title="label"
  />
</template>
