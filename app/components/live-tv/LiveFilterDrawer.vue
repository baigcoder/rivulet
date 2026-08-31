<script setup lang="ts">
import type { LiveCategory } from '~/utils/iptv'
import { mdiClose } from '@mdi/js'

const props = defineProps<{
  show: boolean
  categories: LiveCategory[]
  languages: Array<{ name: string, count: number }>
  selectedCountry: string
  selectedCategory: string
  selectedLanguage: string
  selectedGroup: string
}>()

const emit = defineEmits<{
  close: []
  apply: [filters: { country: string, category: string, language: string, group: string }]
  reset: []
}>()

const localCountry = ref(props.selectedCountry)
const localCategory = ref(props.selectedCategory)
const localLanguage = ref(props.selectedLanguage)
const localGroup = ref(props.selectedGroup)

watch(() => props.show, v => {
  if (v) {
    localCountry.value = props.selectedCountry
    localCategory.value = props.selectedCategory
    localLanguage.value = props.selectedLanguage
    localGroup.value = props.selectedGroup
  }
})

function apply() {
  emit('apply', {
    country: localCountry.value,
    category: localCategory.value,
    language: localLanguage.value,
    group: localGroup.value,
  })
  emit('close')
}

function reset() {
  localCountry.value = ''
  localCategory.value = ''
  localLanguage.value = ''
  localGroup.value = 'All'
  emit('reset')
  emit('close')
}
</script>

<template>
  <teleport to="body">
    <transition name="fade">
      <div v-if="show" class="fixed inset-0 z-50 flex items-end justify-end bg-black/50 md:items-stretch" @click.self="emit('close')">
        <!-- Mobile: bottom sheet -->
        <div class="w-full overflow-y-auto rounded-t-2xl bg-surface-container p-4 md:hidden">
          <div class="mb-4 flex items-center justify-between">
            <h2 class="text-title-large font-bold">
              {{ $t('Filters') }}
            </h2>
            <v-btn :icon="mdiClose" variant="text" size="small" @click="emit('close')" />
          </div>
          <div class="space-y-4">
            <div>
              <label class="mb-1 block text-label-small opacity-50">{{ $t('Language') }}</label>
              <select v-model="localLanguage" class="w-full rounded-lg border border-white/10 bg-surface-container-high px-3 py-2 text-body-medium text-white outline-none">
                <option value="">
                  {{ $t('All Languages') }}
                </option>
                <option v-for="l in languages" :key="l.name" :value="l.name">
                  {{ l.name }} ({{ l.count }})
                </option>
              </select>
            </div>
            <div>
              <label class="mb-1 block text-label-small opacity-50">{{ $t('Group') }}</label>
              <select v-model="localGroup" class="w-full rounded-lg border border-white/10 bg-surface-container-high px-3 py-2 text-body-medium text-white outline-none">
                <option value="All">
                  {{ $t('All Groups') }}
                </option>
                <option v-for="g in ['Sports', 'News', 'Entertainment', 'Kids', 'Movies & Series', 'Music', 'Documentary', 'Religious', 'General']" :key="g" :value="g">
                  {{ g }}
                </option>
              </select>
            </div>
          </div>
          <div class="mt-6 flex gap-3">
            <v-btn variant="tonal" class="flex-1" @click="reset">
              {{ $t('Reset') }}
            </v-btn>
            <v-btn color="primary" class="flex-1" @click="apply">
              {{ $t('Apply') }}
            </v-btn>
          </div>
        </div>
        <!-- Desktop: side drawer -->
        <div class="hidden w-80 overflow-y-auto border-l border-white/10 bg-surface-container p-5 md:block">
          <div class="mb-6 flex items-center justify-between">
            <h2 class="text-title-large font-bold">
              {{ $t('Filters') }}
            </h2>
            <v-btn :icon="mdiClose" variant="text" size="small" @click="emit('close')" />
          </div>
          <div class="space-y-6">
            <div>
              <label class="mb-2 block text-label-small opacity-50">{{ $t('Language') }}</label>
              <select v-model="localLanguage" class="w-full rounded-lg border border-white/10 bg-surface-container-high px-3 py-2 text-body-medium text-white outline-none">
                <option value="">
                  {{ $t('All Languages') }}
                </option>
                <option v-for="l in languages" :key="l.name" :value="l.name">
                  {{ l.name }} ({{ l.count }})
                </option>
              </select>
            </div>
            <div>
              <label class="mb-2 block text-label-small opacity-50">{{ $t('Group') }}</label>
              <select v-model="localGroup" class="w-full rounded-lg border border-white/10 bg-surface-container-high px-3 py-2 text-body-medium text-white outline-none">
                <option value="All">
                  {{ $t('All Groups') }}
                </option>
                <option v-for="g in ['Sports', 'News', 'Entertainment', 'Kids', 'Movies & Series', 'Music', 'Documentary', 'Religious', 'General']" :key="g" :value="g">
                  {{ g }}
                </option>
              </select>
            </div>
          </div>
          <div class="mt-8 flex gap-3">
            <v-btn variant="tonal" class="flex-1" @click="reset">
              {{ $t('Reset') }}
            </v-btn>
            <v-btn color="primary" class="flex-1" @click="apply">
              {{ $t('Apply') }}
            </v-btn>
          </div>
        </div>
      </div>
    </transition>
  </teleport>
</template>

<style scoped>
.fade-enter-active, .fade-leave-active { transition: opacity 200ms ease; }
.fade-enter-from, .fade-leave-to { opacity: 0; }
</style>
