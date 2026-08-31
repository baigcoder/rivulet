<script setup lang="ts">
defineProps<{
  status: string
}>()
</script>

<template>
  <div class="flex items-center gap-2">
    <span
      class="size-2 rounded-full"
      :class="{
        'bg-green-500': status === 'connected',
        'bg-amber-500': status === 'expired',
        'bg-red-500': status === 'disabled',
        'bg-gray-500': status === 'disconnected',
      }"
    />
    <span class="text-body-small">
      {{ status === 'connected' ? $t('Connected') : status === 'expired' ? $t('Expired') : status === 'disabled' ? $t('Disabled') : $t('Disconnected') }}
    </span>
  </div>
</template>
