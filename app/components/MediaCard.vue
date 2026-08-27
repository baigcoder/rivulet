<script setup lang="ts">
import type { Media } from '~/utils/tmdb'
import { mdiBookmark, mdiBookmarkOutline, mdiCheck, mdiCheckboxBlankOutline, mdiCheckboxMarked, mdiClose, mdiEye, mdiEyeOutline, mdiHeart, mdiHeartOutline, mdiPlay, mdiStar } from '@mdi/js'

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

const reserve = computed(() =>
  `auto ${ui.cardWidth}px auto ${Math.round(ui.cardWidth * 1.5) + (props.detail || props.resumeTo ? 44 : 0)}px`)
</script>

<template>
  <nuxt-link
    :to="to ?? mediaLink(media)"
    class="group block select-none outline-none"
    :class="{ '[content-visibility:auto]': !hover }"
    :style="{ containIntrinsicSize: reserve, transition: 'transform 0.25s cubic-bezier(0.4,0,0.2,1), opacity 0.2s ease', transform: removing ? 'scale(0.85)' : pressed ? 'scale(0.92)' : hover ? 'scale(1.08)' : 'scale(1)', opacity: removing ? '0' : '1' }"
    @mouseenter="hover = true; ui.preview(media)"
    @mouseleave="hover = false; pressed = false"
    @focus="hover = true; ui.hover(media)"
    @blur="hover = false"
    @mousedown="pressed = true"
    @mouseup="pressed = false"
  >
    <!-- Poster image container: only the IMAGE is clipped, not the whole box -->
    <div class="relative aspect-2/3 overflow-hidden rounded-lg bg-surface-container">
      <media-poster
        :src="posterUrl(media.poster, ui.posterSize)"
        :alt="media.title"
        class="h-full w-full object-cover"
      />

      <!-- Rating badge — top-left -->
      <div v-if="media.rating" class="absolute left-1.5 top-1.5 flex items-center gap-0.5 rounded-full bg-black/65 px-1.5 py-0.5 text-label-small text-white">
        <svg viewBox="0 0 24 24" class="size-3 fill-amber-400"><path :d="mdiStar" /></svg>
        {{ media.rating.toFixed(1) }}
      </div>

      <!-- Watched tick — top-right -->
      <div v-if="watched" class="absolute right-1.5 top-1.5 grid size-5 place-items-center rounded-full bg-primary text-on-primary">
        <svg viewBox="0 0 24 24" class="size-3.5 fill-current"><path :d="mdiCheck" /></svg>
      </div>

      <!-- Remove (X) badge — top-right, always visible on Continue Watching -->
      <v-btn
        v-if="onRemove && !selectable"
        icon
        size="x-small"
        variant="flat"
        color="black"
        class="absolute right-1 top-1 z-10 opacity-90 hover:opacity-100 focus-visible:opacity-100"
        :class="removing ? 'scale-0 opacity-0' : 'scale-100'"
        :style="{ transition: 'transform 0.25s cubic-bezier(0.4,0,0.2,1), opacity 0.2s ease' }"
        @click.stop.prevent="handleRemove()"
      >
        <v-icon :icon="mdiClose" size="14" color="white" />
        <v-tooltip activator="parent" :text="$t('Remove from list')" placement="left" />
      </v-btn>

      <!-- Selection checkbox — top-right -->
      <div
        v-if="selectable"
        class="absolute right-1.5 top-1.5 z-10 grid size-6 place-items-center rounded-md transition-all duration-150"
        :class="selected ? 'bg-primary shadow-lg shadow-primary/30' : 'bg-black/50 hover:bg-black/70'"
        @click.stop.prevent="emit('toggleSelect')"
      >
        <v-icon
          :icon="selected ? mdiCheckboxMarked : mdiCheckboxBlankOutline"
          size="16"
          :color="selected ? 'white' : 'white'"
        />
      </div>

      <!-- Hover overlay -->
      <transition
        enter-active-class="transition-all duration-200"
        leave-active-class="transition-all duration-150"
        enter-from-class="opacity-0"
        leave-to-class="opacity-0"
      >
        <div
          v-if="hover"
          class="absolute inset-0 flex flex-col bg-gradient-to-t from-black/95 via-black/40 to-black/60"
        >
          <!-- Action buttons: top-left (X is top-right) -->
          <div class="flex shrink-0 justify-start gap-1 p-2 pb-0">
            <v-btn icon size="x-small" variant="flat" color="white" tabindex="-1" @click.stop.prevent="library.toggleFavourite(media)">
              <v-icon :icon="library.isFavourite(media) ? mdiHeart : mdiHeartOutline" size="14" :class="library.isFavourite(media) ? 'text-red' : 'text-black'" />
              <v-tooltip activator="parent" :text="library.isFavourite(media) ? $t('Remove from favourites') : $t('Favourite')" />
            </v-btn>
            <v-btn icon size="x-small" variant="flat" color="white" tabindex="-1" @click.stop.prevent="library.toggleWatchlist(media)">
              <v-icon :icon="library.inWatchlist(media) ? mdiBookmark : mdiBookmarkOutline" size="14" :class="library.inWatchlist(media) ? 'text-purple' : 'text-black'" />
              <v-tooltip activator="parent" :text="library.inWatchlist(media) ? $t('Remove from watchlist') : $t('Add to watchlist')" />
            </v-btn>
            <v-btn icon size="x-small" variant="flat" color="white" tabindex="-1" @click.stop.prevent="library.toggleWatched(media)">
              <v-icon :icon="watched ? mdiEye : mdiEyeOutline" size="14" :class="watched ? 'text-purple' : 'text-black'" />
              <v-tooltip activator="parent" :text="watched ? $t('Mark unwatched') : $t('Mark watched')" />
            </v-btn>
          </div>

          <!-- Play button centred via flex spacer -->
          <div class="flex flex-1 items-center justify-center">
            <nuxt-link
              v-if="resumeTo"
              :to="resumeTo"
              class="grid size-10 place-items-center rounded-full bg-white shadow-xl transition-transform duration-150 hover:scale-110"
              @click.stop.prevent
            >
              <v-icon :icon="mdiPlay" size="22" color="black" class="ml-0.5" />
            </nuxt-link>
          </div>

          <!-- Bottom spacer for gradient -->
          <div class="h-8 shrink-0" />
        </div>
      </transition>

      <!-- Progress bar — absolute bottom -->
      <div v-if="played > 0 && !progress?.watched" class="absolute inset-x-0 bottom-0 z-[5]">
        <div v-if="episode" class="px-1.5 pb-0.5 text-label-small font-medium text-white drop-shadow-[0_1px_3px_rgba(0,0,0,0.9)]">
          {{ episode }}
        </div>
        <div class="h-1 bg-black/50">
          <div class="h-full rounded-r-full bg-primary transition-[width] duration-300" :style="{ width: `${played * 100}%` }" />
        </div>
      </div>

      <!-- Focus ring -->
      <div class="pointer-events-none absolute inset-0 rounded-lg opacity-0 ring-2 ring-inset ring-primary/50 transition-opacity duration-150 group-focus-visible:opacity-100" />
    </div>

    <!-- Title + year: OUTSIDE poster box, never clipped by overflow-hidden -->
    <div v-if="resumeTo || detail" class="px-1 pt-1.5">
      <div class="line-clamp-2 text-title-small font-medium leading-tight">
        {{ media.title }}
      </div>
      <div class="text-body-small opacity-55">
        {{ media.year || $t('unknown') }}
      </div>
    </div>
  </nuxt-link>
</template>
