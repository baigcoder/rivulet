<script setup lang="ts">
/**
 * One category, deep-linkable.
 *
 * The parent `free.vue` keeps this URL on the unified browse shell and
 * reads `route.params.category` — the same job Premium's category page
 * does by mounting the browser. This file exists so the route is
 * registered; it is not mounted (the parent does not swap to `<nuxt-page>`
 * for category).
 */
definePageMeta({ layout: 'default' })
</script>

<template>
  <div />
</template>
