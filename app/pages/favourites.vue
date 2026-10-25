<script setup lang="ts">
import { mdiHeartOutline } from '@mdi/js'

const library = useLibraryStore()
</script>

<template>
  <library-browser :title="$t('Favourites')" :empty-icon="mdiHeartOutline" :empty-title="$t('No favourites yet')" :empty-hint="$t('Press the heart on anything you love and it collects here.')" :items="library.favouriteList" :recent="$t('Recently added')" />
</template>
