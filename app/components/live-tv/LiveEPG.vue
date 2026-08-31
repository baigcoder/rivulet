<script setup lang="ts">
import type { EpgProgram } from '~/utils/iptv'

defineProps<{
  programs: EpgProgram[]
}>()
</script>

<template>
  <div v-if="programs.length > 0" class="space-y-1">
    <div v-for="(prog, i) in programs.slice(0, 2)" :key="i" class="text-body-small">
      <span class="font-medium text-primary">{{ prog.title }}</span>
      <span v-if="prog.start" class="ml-1 opacity-50">{{ prog.start }}</span>
    </div>
  </div>
  <div v-else class="text-body-small opacity-40">
    {{ $t('EPG unavailable') }}
  </div>
</template>
