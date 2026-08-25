<script setup lang="ts">
const library = useLibraryStore()
</script>

<template>
  <library-browser :title="$t('History')" :items="library.history" :recent="$t('Last watched')" />
</template>
