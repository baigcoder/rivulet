<script setup lang="ts">
import { mdiClose, mdiMagnify } from '@mdi/js'

const model = defineModel<string>({ default: '' })

const inputEl = ref<HTMLInputElement>()

function clear() {
  model.value = ''
}
</script>

<template>
  <v-text-field
    ref="inputEl"
    :model-value="model"
    :label="$t('Search channels...')"
    :prepend-inner-icon="mdiMagnify"
    :append-inner-icon="model ? mdiClose : undefined"
    variant="outlined"
    density="comfortable"
    clearable
    hide-details
    @update:model-value="model = $event ?? ''"
    @click:append-inner="clear"
  />
</template>
