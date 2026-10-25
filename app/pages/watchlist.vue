<script setup lang="ts">
import { mdiBookmarkOutline } from '@mdi/js'

const library = useLibraryStore()
</script>

<template>
  <library-browser :title="$t('Watchlist')" :empty-icon="mdiBookmarkOutline" :empty-title="$t('Your watchlist is empty')" :empty-hint="$t('Press the bookmark on any film or show to save it here for later.')" :items="library.watchlistItems" :recent="$t('Recently added')" />
</template>
