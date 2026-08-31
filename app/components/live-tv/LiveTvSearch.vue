<script setup lang="ts">
import { mdiClose, mdiMagnify } from '@mdi/js'

const model = defineModel<string>({ default: '' })

const inputRef = ref<HTMLInputElement>()

function focus() {
  inputRef.value?.focus()
}

defineExpose({ focus })
</script>

<template>
  <div class="group relative">
    <!-- Glow aura behind search bar when focused -->
    <div class="absolute -inset-0.5 rounded-2xl bg-gradient-to-r from-primary/40 via-purple-500/30 to-primary/40 opacity-0 blur-md transition-all duration-300 group-focus-within:opacity-100" />

    <div
      class="relative flex items-center rounded-2xl border border-white/12 bg-black/40 backdrop-blur-xl px-4 py-3 shadow-lg transition-all duration-300 group-focus-within:border-primary/60 group-focus-within:bg-black/60 group-focus-within:shadow-xl"
    >
      <!-- Search icon inside stylized pill -->
      <div class="me-3 grid size-8 shrink-0 place-items-center rounded-xl bg-primary/15 transition-all duration-300 group-focus-within:bg-primary group-focus-within:text-on-primary">
        <v-icon
          :icon="mdiMagnify"
          size="19"
          class="text-primary transition-colors group-focus-within:text-on-primary"
        />
      </div>

      <input
        ref="inputRef"
        v-model="model"
        type="text"
        data-live-search
        :placeholder="$t('Search channels, programs, countries...')"
        class="w-full border-none bg-transparent text-body-large font-medium text-white outline-none ring-0 shadow-none placeholder:text-white/40 focus:border-none focus:outline-none focus:ring-0"
      >

      <!-- Clear search button -->
      <transition name="fade">
        <button
          v-if="model"
          type="button"
          class="ms-2 grid size-7 shrink-0 place-items-center rounded-full bg-white/10 text-white/70 transition-all hover:bg-white/20 hover:text-white"
          :aria-label="$t('Clear search')"
          @click="model = ''"
        >
          <v-icon :icon="mdiClose" size="14" />
        </button>
      </transition>

      <!-- Keyboard shortcut hint when input is empty -->
      <div v-if="!model" class="ms-2 hidden items-center gap-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-[11px] font-semibold text-white/40 sm:flex">
        <span>Ctrl</span>
        <span>K</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 150ms cubic-bezier(0.4, 0, 0.2, 1), transform 150ms cubic-bezier(0.4, 0, 0.2, 1);
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
  transform: scale(0.75);
}
</style>
