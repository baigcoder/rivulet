<script setup lang="ts">
import type { Media } from '~/utils/tmdb'
import { mdiCheck, mdiCheckboxBlankOutline, mdiCheckboxMarked, mdiClose, mdiPlay, mdiStar } from '@mdi/js'

const props = defineProps<{
  media: Media
  /** Show the title and year under the poster. */
  detail?: boolean
  /** Overrides the detail page as the destination — the resume rows play directly. */
  to?: string
  /** Deep link for the "Resume" item in the 3-dot menu. */
  resumeTo?: string
  /** Called when the user removes this card from a list (e.g. Continue Watching). */
  onRemove?: () => void
  /** Enable selection mode with checkboxes. */
  selectable?: boolean
  /** Whether this card is currently selected. */
  selected?: boolean
}>()

const emit = defineEmits<{ toggleSelect: [] }>()

const ui = useUiStore()
const library = useLibraryStore()

const progress = computed(() => library.cardProgress(props.media))
const played = computed(() => fraction(progress.value))
const episode = computed(() => library.cardLabel(props.media))
const watched = computed(() => library.isWatched(props.media))

const hover = ref(false)
const removing = ref(false)
const pressed = ref(false)

function handleRemove() {
  removing.value = true
  setTimeout(() => props.onRemove?.(), 250)
}

function handleClick(e: MouseEvent) {
  if (removing.value) {
    e.preventDefault()
    e.stopPropagation()
  }
}

const reserve = computed(() => `auto ${ui.cardWidth}px auto ${Math.round(ui.cardWidth * 1.5)}px`)

const isResume = computed(() => !!props.onRemove && !props.selectable)

const cardStyle = computed(() => {
  if (isResume.value) {
    return {
      transform: removing.value ? 'scale(0.9)' : 'scale(1)',
      transition: 'transform 0.3s cubic-bezier(0.22, 1, 0.36, 1), opacity 0.25s ease',
      opacity: removing.value ? '0' : '1',
    }
  }
  const scale = removing.value ? '0.85' : pressed.value ? '0.92' : hover.value ? '1.05' : '1'
  const shadow = hover.value && !removing.value
    ? '0 12px 28px rgba(0,0,0,0.35), 0 0 0 1px rgba(255,255,255,0.08)'
    : '0 2px 8px rgba(0,0,0,0.15)'
  return {
    transform: `scale(${scale})`,
    boxShadow: shadow,
    transition: 'transform 0.3s cubic-bezier(0.22, 1, 0.36, 1), opacity 0.25s ease',
    opacity: removing.value ? '0' : '1',
  }
})
</script>

<template>
  <div
    class="group relative block select-none outline-none"
    :class="{ '[content-visibility:auto]': !hover }"
    :style="{ containIntrinsicSize: reserve, ...cardStyle }"
    @mouseenter="hover = true; ui.preview(media)"
    @mouseleave="hover = false; pressed = false"
    @focus="hover = true; ui.hover(media)"
    @blur="hover = false"
    @mousedown="pressed = true"
    @mouseup="pressed = false"
  >
    <!-- Poster image container: only the IMAGE is clipped, not the whole box -->
    <div class="relative aspect-2/3 overflow-hidden rounded-lg bg-surface-container">
      <!-- Navigation link covers entire poster, sits below interactive elements -->
      <nuxt-link
        :to="to ?? mediaLink(media)"
        class="absolute inset-0 z-10"
        @click="handleClick"
      />

      <media-poster
        :src="posterUrl(media.poster, ui.posterSize)"
        :alt="media.title"
        class="h-full w-full object-cover"
      />

      <!-- Watched tick — top-right -->
      <div v-if="watched" class="absolute right-1.5 top-1.5 grid size-5 place-items-center rounded-full bg-primary text-on-primary z-20">
        <svg viewBox="0 0 24 24" class="size-3.5 fill-current"><path :d="mdiCheck" /></svg>
      </div>

      <!-- Remove (X) badge — top-right, always visible on Continue Watching.
           Above the navigation link so clicks don't navigate. -->
      <v-btn
        v-if="onRemove && !selectable"
        icon
        size="28"
        variant="flat"
        color="white"
        class="absolute right-1 top-1 z-30 rounded-full transition-[transform,opacity] duration-200"
        :class="removing ? 'scale-0 opacity-0 pointer-events-none' : hover ? 'opacity-100 shadow-lg shadow-black/30' : 'opacity-0 group-hover:opacity-90 focus-visible:opacity-100'"
        :style="{ transition: 'transform 0.3s cubic-bezier(0.22, 1, 0.36, 1), opacity 0.25s ease, box-shadow 0.25s ease' }"
        @pointerdown.stop
        @click.stop.prevent="handleRemove()"
      >
        <v-icon :icon="mdiClose" size="16" class="text-black" />
        <v-tooltip activator="parent" :text="$t('Remove from list')" placement="left" />
      </v-btn>

      <!-- Selection checkbox — top-right -->
      <div
        v-if="selectable"
        class="absolute right-1.5 top-1.5 z-30 grid size-6 place-items-center rounded-md transition-colors duration-150"
        :class="selected ? 'bg-primary shadow-lg shadow-primary/30' : 'bg-black/50 hover:bg-black/70'"
        @click.stop.prevent="emit('toggleSelect')"
      >
        <v-icon
          :icon="selected ? mdiCheckboxMarked : mdiCheckboxBlankOutline"
          size="16"
          color="white"
        />
      </div>

      <!-- Progress bar — absolute bottom -->
      <div v-if="played > 0 && !progress?.watched" class="absolute inset-x-0 bottom-0 z-[5]">
        <div v-if="episode" class="px-1.5 pb-0.5 text-label-small font-medium text-white drop-shadow-[0_1px_3px_rgba(0,0,0,0.9)]">
          {{ episode }}
        </div>
        <div class="h-1 bg-black/50">
          <div class="h-full rounded-r-full bg-primary transition-[width] duration-300" :style="{ width: `${played * 100}%` }" />
        </div>
      </div>

      <!-- Hover overlay: play icon + title + year/rating on the poster -->
      <transition
        enter-active-class="transition-[opacity] duration-250 ease-out"
        leave-active-class="transition-[opacity] duration-200 ease-in"
        enter-from-class="opacity-0"
        leave-to-class="opacity-0"
      >
        <div
          v-if="hover && !removing"
          class="absolute inset-0 z-20 flex flex-col items-center justify-center bg-black/50"
        >
          <nuxt-link
            :to="resumeTo || to || mediaLink(media)"
            class="grid size-14 place-items-center rounded-full bg-white shadow-xl transition-[transform,box-shadow] duration-200 hover:scale-110 hover:shadow-[0_4px_24px_rgba(255,255,255,0.35)]"
            @pointerdown.stop
          >
            <v-icon :icon="mdiPlay" size="30" class="ml-0.5 text-black" />
          </nuxt-link>
          <div class="mt-2 text-center">
            <div class="line-clamp-1 px-2 text-title-small font-semibold leading-tight text-white drop-shadow-[0_1px_4px_rgba(0,0,0,0.8)]">
              {{ media.title }}
            </div>
            <div class="mt-0.5 flex items-center justify-center gap-1 text-body-small text-white/80 drop-shadow-[0_1px_3px_rgba(0,0,0,0.7)]">
              <span v-if="media.year">{{ media.year }}</span>
              <span v-if="media.rating" class="flex items-center gap-0.5">
                <svg viewBox="0 0 24 24" class="size-3 fill-amber-400"><path :d="mdiStar" /></svg>
                {{ media.rating.toFixed(1) }}
              </span>
            </div>
          </div>
        </div>
      </transition>

      <!-- Focus ring -->
      <div class="pointer-events-none absolute inset-0 rounded-lg opacity-0 ring-2 ring-inset ring-primary/50 transition-opacity duration-250 group-focus-visible:opacity-100 z-20" />
    </div>
  </div>
</template>
