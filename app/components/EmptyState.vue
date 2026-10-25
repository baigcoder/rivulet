<script setup lang="ts">
/**
 * What a list says when it has nothing in it. A dim icon and "Nothing here."
 * left the next move to guesswork in the middle of an empty screen; this names
 * what the list is for and offers the way to fill it — buttons, never a bare
 * sentence, so a remote can take the suggestion as easily as a mouse.
 */
defineProps<{
  icon: string
  title: string
  hint?: string
}>()
</script>

<template>
  <div class="mx-auto flex max-w-md flex-col items-center gap-3 px-4 py-12 text-center">
    <div class="grid size-20 place-items-center rounded-full bg-surface-container-high">
      <v-icon :icon="icon" size="40" class="text-primary" />
    </div>
    <p class="text-title-medium">
      {{ title }}
    </p>
    <p v-if="hint" class="text-body-medium text-medium-emphasis">
      {{ hint }}
    </p>
    <div v-if="$slots.default" class="mt-2 flex flex-wrap justify-center gap-2">
      <slot />
    </div>
  </div>
</template>
