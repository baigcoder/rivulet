<script setup lang="ts">
import { mdiHistory } from '@mdi/js'

const library = useLibraryStore()
</script>

<template>
  <!-- One entry per title rather than per episode — a show you are ten episodes
       into should take up one card, not ten. -->
  <library-browser :title="$t('History')" :empty-icon="mdiHistory" :empty-title="$t('Nothing watched yet')" :empty-hint="$t('Whatever you play shows up here, with where you stopped.')" :items="library.history" :recent="$t('Last watched')" />
</template>
