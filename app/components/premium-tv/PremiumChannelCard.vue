<script setup lang="ts">
/**
 * One channel in the Premium grid.
 *
 * The Free TV card cannot be reused: a free channel arrives with its
 * `streamUrl` on it and a Premium channel never does — the URL is minted
 * per play by the Rust side and expires — and Premium EPG times are epoch
 * **seconds** where Free TV's are ISO strings. Every playability check and
 * every clock arithmetic in the two cards therefore differs, which is the
 * whole of the card. What they do share is the shape, and that is
 * deliberate: the two live sections should not look like two products.
 *
 * **The card is deliberately short.** A lineup is thousands of channels
 * and the thing a viewer is doing is *finding one*, so the card carries
 * the four things that identify a channel — its logo, that it is live,
 * its name, and where it comes from — and nothing else. The guide is one
 * hairline of progress; the programme title, the next programme and the
 * clock live on the player and in the channel drawer, where there is one
 * channel to read about instead of five hundred.
 */
import type { EpgProgram, IPTVChannel } from '~/types/premium'
import { mdiPlay, mdiStar } from '@mdi/js'
import { computed, onUnmounted, ref, watch } from 'vue'

const props = defineProps<{
  channel: IPTVChannel
  /** Now/next for this channel, read through the store's cache. */
  nowNext: (id: string) => { now: EpgProgram | null, next: EpgProgram | null }
  favorite: (id: string) => boolean
  compact?: boolean
}>()

const emit = defineEmits<{
  play: [channel: IPTVChannel]
  toggleFavorite: [channel: IPTVChannel]
}>()

const guide = computed(() => props.nowNext(props.channel.id))
const nowProgram = computed(() => guide.value.now)
const fav = computed(() => props.favorite(props.channel.id))
const imgError = ref(false)
const imgLoaded = ref(false)

/**
 * A minute is the resolution a progress bar needs, and the interval only
 * exists while there is a programme to measure. Five thousand cards each
 * holding an idle timer was measurable on a TV all by itself.
 */
const nowSecs = ref(Math.floor(Date.now() / 1000))
let progressTimer: ReturnType<typeof setInterval> | undefined
watch(nowProgram, prog => {
  if (prog && !progressTimer) {
    progressTimer = setInterval(() => {
      nowSecs.value = Math.floor(Date.now() / 1000)
    }, 60_000)
  }
  else if (!prog && progressTimer) {
    clearInterval(progressTimer)
    progressTimer = undefined
  }
}, { immediate: true })
onUnmounted(() => {
  if (progressTimer !== undefined)
    clearInterval(progressTimer)
})

const epgProgress = computed(() => {
  const prog = nowProgram.value
  if (!prog)
    return 0
  // A programme with no listed end is assumed to be an hour long. XMLTV
  // allows the omission and providers use it; the alternative is a bar
  // that is either always empty or always full.
  const end = prog.stop ?? prog.start + 3600
  if (end <= prog.start)
    return 0
  const pct = ((nowSecs.value - prog.start) / (end - prog.start)) * 100
  return Math.max(0, Math.min(100, Math.round(pct)))
})

/**
 * The quality token, lifted out of the name and into a badge.
 *
 * Providers write the bitrate into the channel name — `beIN SPORTS 1 FHD`
 * — and there are two thousand of them in one lineup, which is why the
 * grid reads as a wall of near-duplicates. It is not duplication: `HD`
 * and `FHD` are two streams, and the badge is what makes that legible in
 * a glance instead of at the end of a truncated line. Package tokens
 * (`VIP`, `MULTI`) are the provider's bookkeeping and go entirely.
 */
const QUALITY = ['4K', 'UHD', 'FHD', 'HEVC', 'HD', 'SD'] as const
const PACKAGE = /^(?:VIP|MULTI)$/i

const parsedName = computed(() => {
  const raw = (props.channel.name || '').trim()
  let quality = ''
  const kept: string[] = []
  for (const word of raw.split(/\s+/)) {
    const bare = word.replace(/^[[(]|[\])]$/g, '')
    const hit = QUALITY.find(q => q.toLowerCase() === bare.toLowerCase())
    if (hit) {
      // Highest wins: `HD FHD` in one name means the FHD stream.
      if (!quality || QUALITY.indexOf(hit) < QUALITY.indexOf(quality as typeof QUALITY[number]))
        quality = hit
      continue
    }
    if (PACKAGE.test(bare))
      continue
    kept.push(word)
  }
  // Never end up with an empty card: a name that was *only* a quality
  // token keeps it.
  const name = kept.join(' ').replace(/\s{2,}/g, ' ').trim()
  return { name: name || raw || $t('Channel'), quality }
})

const displayName = computed(() => parsedName.value.name)
const quality = computed(() => parsedName.value.quality)

const channelInitials = computed(() => {
  const clean = displayName.value.replace(/[^a-z0-9\s]/gi, ' ').trim()
  const words = clean
    .split(/\s+/)
    .filter(w => w.length > 0 && !/^(?:tv|channel|live)$/i.test(w))
  const first = words[0] || clean
  const second = words[1]
  if (first && second && first[0] && second[0])
    return (first[0] + second[0]).toUpperCase()
  const str = first || clean || 'TV'
  return str.slice(0, 2).toUpperCase()
})

/** A null, empty or placeholder logo is common enough to be the norm. */
const hasLogo = computed(() => {
  const url = props.channel.logoUrl
  if (!url || url === 'null' || url === 'undefined')
    return false
  return url.length >= 8
})

/** Where the channel is from: the provider's own answer, else its group. */
const subtitle = computed(() => props.channel.country || props.channel.categoryName || '')
</script>

<template>
  <!--
    A real `button`, not a div with a click handler: it is the one thing
    that gives the remote a focus ring, Enter and Space for free. Every
    `hover:` below has a `focus-visible:` twin for the same reason.
  -->
  <button
    type="button"
    class="group relative flex flex-col overflow-hidden rounded-xl bg-surface-container-high/70 text-start ring-1 ring-white/5 transition-colors duration-150 hover:bg-surface-container-highest hover:ring-primary/50 focus-visible:bg-surface-container-highest focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
    :aria-label="displayName"
    @click="emit('play', channel)"
  >
    <!--
      Fixed height, `object-contain`: a lineup's logos are every aspect
      ratio there is, and a box that sizes itself to them makes a grid of
      ragged rows. The height is the one thing every card agrees on.
    -->
    <div
      class="relative w-full shrink-0 overflow-hidden bg-black/25"
      :class="compact ? 'h-14' : 'h-[68px]'"
    >
      <!-- Skeleton, not a blank: a slow logo should look like it is
           coming, and the shimmer is one element inside a bounded box. -->
      <div
        v-if="hasLogo && !imgLoaded && !imgError"
        class="absolute inset-0 animate-pulse bg-white/5"
      />
      <img
        v-if="hasLogo && !imgError"
        :src="channel.logoUrl!"
        alt=""
        loading="lazy"
        decoding="async"
        class="size-full object-contain p-2 transition-opacity duration-200"
        :class="imgLoaded ? 'opacity-100' : 'opacity-0'"
        @load="imgLoaded = true"
        @error="imgError = true"
      >

      <!-- No logo, or one that 404'd. Initials rather than a broken-image
           glyph, which is the same information and looks like a bug. -->
      <div
        v-if="!hasLogo || imgError"
        class="grid size-full place-items-center"
      >
        <span class="text-title-small font-black tracking-wider text-white/45">
          {{ channelInitials }}
        </span>
      </div>

      <!-- Decorative overlay control: `tabindex="-1"` so the remote walks
           channels and not two stops per card. The favourite is reachable
           from the player and the channel row instead. -->
      <span
        class="absolute right-1 top-1 grid size-6 cursor-pointer place-items-center rounded-full bg-black/60 transition-opacity hover:bg-black/80 group-focus-visible:opacity-100 group-hover:opacity-100"
        :class="fav ? 'opacity-100' : 'opacity-0'"
        role="button"
        tabindex="-1"
        :aria-label="fav ? $t('Remove from favorites') : $t('Add to favorites')"
        @click.stop.prevent="emit('toggleFavorite', channel)"
      >
        <v-icon :icon="mdiStar" size="14" :class="fav ? 'text-amber-400' : 'text-white/70'" />
      </span>

      <div class="absolute inset-0 grid place-items-center bg-black/35 opacity-0 transition-opacity duration-150 group-focus-visible:opacity-100 group-hover:opacity-100">
        <div class="grid size-9 place-items-center rounded-full bg-primary shadow-lg shadow-primary/40">
          <v-icon :icon="mdiPlay" size="20" class="ml-0.5 text-on-primary" />
        </div>
      </div>
    </div>

    <div class="flex min-w-0 flex-1 flex-col px-2.5 pb-2 pt-1.5">
      <!-- One quiet row for status. A red pill on every one of five
           thousand cards is not information, it is texture — the dot says
           the same thing in a tenth of the ink, and the quality badge
           beside it is what actually distinguishes two rows. -->
      <div class="flex items-center gap-1.5">
        <span class="size-1.5 shrink-0 rounded-full bg-red-500" aria-hidden="true" />
        <span class="text-[9px] font-semibold uppercase tracking-widest text-white/40">
          {{ $t('Live') }}
        </span>
        <span
          v-if="quality"
          class="ml-auto rounded bg-white/8 px-1 text-[9px] font-bold tracking-wide text-white/55"
        >{{ quality }}</span>
      </div>

      <h3 class="truncate text-body-small font-semibold leading-tight" :title="displayName">
        {{ displayName }}
      </h3>

      <p v-if="subtitle" class="truncate text-[10px] leading-tight text-white/35">
        {{ subtitle }}
      </p>

      <!-- The guide, reduced to the only part of it that reads at this
           size: how far through the programme we are. No title, no
           clock, no container when there is no guide at all. -->
      <div v-if="nowProgram" class="mt-1.5 h-0.5 w-full overflow-hidden rounded-full bg-white/8">
        <div class="h-full rounded-full bg-primary/70" :style="{ width: `${epgProgress}%` }" />
      </div>
    </div>
  </button>
</template>
