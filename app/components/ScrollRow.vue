<script setup lang="ts">
import { mdiArrowRight, mdiChevronLeft, mdiChevronRight } from '@mdi/js'

/**
 * A titled horizontal strip. The arrows page it; a wheel with Shift held, or a
 * trackpad sideways, scrolls it natively — there is no scrollbar to grab.
 */
const props = defineProps<{ title: string, count?: number, canLoad?: boolean, /** Deep link for the header's 'See all' chip. */ to?: string }>()

const emit = defineEmits<{ end: [] }>()

const scroller = ref<HTMLElement | null>(null)
const track = ref<HTMLElement | null>(null)

const atStart = ref(true)
const atEnd = ref(true)
const overflows = ref(false)

/**
 * Where the row is sitting. Measured here rather than by `useScroll`, which
 * looks on mount and on scroll and nothing else: a row is empty when it mounts,
 * so both arrows came up disabled and stayed that way until it was dragged by
 * hand. The observer on the track is what notices the cards arriving — and the
 * poster size changing, which moves the ends just as much.
 */
function measure() {
  const el = scroller.value
  if (!el)
    return
  const max = el.scrollWidth - el.clientWidth
  overflows.value = max > 1
  atStart.value = el.scrollLeft < 1
  atEnd.value = el.scrollLeft > max - 1
}

useResizeObserver([scroller, track], measure)

/**
 * A row that is moving belongs to nobody's pointer. Cards passing under a
 * stationary cursor fire an enter and a leave each, and every one of those
 * mounts the hover overlay's dozen components, moves the backdrop and takes the
 * card out of `content-visibility` — a whole row of that for one flick of the
 * wheel, which is what the flicker and the stutter were.
 *
 * The class goes on the track and not on the scroller: a scroller that ignores
 * the pointer never sees the wheel either, and the page would scroll instead.
 */
const gliding = ref(false)
const settle = useDebounceFn(() => (gliding.value = false), 140)

useEventListener(scroller, 'scroll', () => {
  measure()
  gliding.value = true
  settle()
}, { passive: true })

useInfiniteScroll(scroller, () => emit('end'), {
  distance: 600,
  direction: 'right',
  canLoadMore: () => props.canLoad === true,
})

// Paging by a fraction of the width leaves a sliver of a poster at each edge and
// the next press compounds it. Whole cards land where the row started: the step
// is a multiple of the pitch, so every stop insets a card by the track's own
// padding, exactly as the first one is.
function page(direction: 1 | -1) {
  const el = scroller.value
  if (!el)
    return

  const gap = 16 // gap-4
  const card = (track.value?.firstElementChild as HTMLElement | null)?.offsetWidth ?? el.clientWidth
  const cards = Math.max(1, Math.floor(el.clientWidth / (card + gap)))
  el.scrollBy({ left: direction * cards * (card + gap), behavior: 'smooth' })
}
</script>

<template>
  <section class="group/row">
    <div class="mx-4 flex items-center gap-3 px-1 pb-3 pt-1 md:mx-6">
      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2">
          <h2 class="truncate text-title-large font-bold tracking-tight text-on-surface">
            {{ title }}
          </h2>
          <span
            v-if="count !== undefined"
            class="rounded-full bg-white/8 px-2 py-0.5 text-label-small font-semibold text-white/55 ring-1 ring-white/8"
          >
            {{ count.toLocaleString() }}
          </span>
        </div>
      </div>

      <!-- Paging arrows — visible on hover/focus, always shown on keyboard nav -->
      <div
        v-if="overflows"
        class="hidden items-center gap-0.5 opacity-0 transition-opacity group-hover/row:opacity-100 group-focus-within/row:opacity-100 md:flex"
      >
        <button
          tabindex="-1"
          :disabled="atStart"
          class="grid size-8 place-items-center rounded-lg text-white/50 transition-colors hover:bg-white/8 hover:text-white disabled:pointer-events-none disabled:opacity-25"
          @click="page(-1)"
        >
          <v-icon :icon="mdiChevronLeft" size="20" />
        </button>
        <button
          tabindex="-1"
          :disabled="atEnd"
          class="grid size-8 place-items-center rounded-lg text-white/50 transition-colors hover:bg-white/8 hover:text-white disabled:pointer-events-none disabled:opacity-25"
          @click="page(1)"
        >
          <v-icon :icon="mdiChevronRight" size="20" />
        </button>
      </div>

      <!-- "See all" link -->
      <a
        v-if="to"
        :href="localePath(to)"
        class="inline-flex shrink-0 items-center gap-1.5 rounded-xl border border-white/10 bg-white/6 px-3 py-1.5 text-label-medium font-semibold text-white/80 transition-colors hover:border-primary/40 hover:bg-primary/12 hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        @click.stop.prevent="navigateTo(localePath(to))"
      >
        {{ $t('See all') }}
        <v-icon :icon="mdiArrowRight" size="15" />
      </a>
    </div>

    <!-- No scroll-snap. It re-animates after every wheel notch and again each
         time a page of cards is appended mid-scroll, which is the jerk you feel
         holding Shift — and `page()` already lands where snap wanted to be.
         scroll-px: keeps the focused card off the edge when the d-pad scrolls
         the row to it. overscroll-x-contain: reaching the end of a row must not
         hand the rest of the gesture to the page behind it. -->
    <div ref="scroller" class="overflow-x-auto scroll-px-4 overscroll-x-contain md:scroll-px-6 no-scrollbar">
      <div ref="track" class="w-max flex gap-3 px-4 pb-5 pt-3 md:px-6" :class="{ 'pointer-events-none': gliding }">
        <slot />
      </div>
    </div>
  </section>
</template>
