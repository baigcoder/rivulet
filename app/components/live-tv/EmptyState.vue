<script setup lang="ts">
import { mdiTelevision } from '@mdi/js'

defineProps<{
  message?: string
}>()
</script>

<template>
  <div class="flex flex-col items-center py-12 text-center">
    <v-icon :icon="mdiTelevision" size="56" class="mb-4 opacity-15" />
    <p class="text-body-medium opacity-60">
      {{ message || $t('No channels found') }}
    </p>
    <slot />
  </div>
</template>
