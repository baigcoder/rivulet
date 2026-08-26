<script setup lang="ts">
// The Home row headers deep-link here pre-filtered (?cat=…, ?lang=…); the
// :key forces a fresh browser when the query changes under keepalive.
const route = useRoute()
const initialCategory = computed(() => String(route.query.cat ?? '') || undefined)
const lang = computed(() => String(route.query.lang ?? '') || undefined)

definePageMeta({ keepalive: true })
</script>

<template>
  <media-browser
    :key="route.fullPath"
    type="tv"
    :initial-category="initialCategory"
    :extra-params="{ with_original_language: lang }"
  />
</template>
