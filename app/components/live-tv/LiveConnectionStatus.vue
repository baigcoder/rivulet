<script setup lang="ts">
import { mdiCheckCircle, mdiLoading } from '@mdi/js'

const liveTv = useLiveTvStore()

const online = ref(navigator.onLine)

onMounted(() => {
  window.addEventListener('online', () => {
    online.value = true
  })
  window.addEventListener('offline', () => {
    online.value = false
  })
})
</script>

<template>
  <div
    v-if="liveTv.activeSourceId"
    class="flex items-center gap-1.5"
    :title="online ? $t('Connected') : $t('Offline')"
  >
    <v-icon
      :icon="mdiCheckCircle"
      size="8"
      :class="online ? 'text-green-500' : 'text-red-500'"
    />
    <span v-if="liveTv.dashboardLoading" class="flex items-center gap-1 text-label-small text-primary">
      <v-icon :icon="mdiLoading" size="12" class="animate-spin" />
    </span>
  </div>
</template>
