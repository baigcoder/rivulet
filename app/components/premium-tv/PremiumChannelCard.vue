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
    class="group relative flex flex-col overflow-hidden rounded-2xl bg-surface-container-high text-start ring-1 ring-white/6 cursor-pointer hover:-translate-y-0.5 hover:ring-primary/50 hover:shadow-xl hover:shadow-primary/10 focus-visible:-translate-y-0.5 transition-[transform,box-shadow] duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
    :aria-label="displayName"
    @click="emit('play', channel)"
  >
    <!-- Logo viewport — aspect-video to match Free TV -->
    <div class="relative aspect-video w-full overflow-hidden bg-surface-container">
      <!-- Skeleton -->
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
        class="size-full object-contain p-3 transition-opacity duration-300"
        :class="imgLoaded ? 'opacity-100' : 'opacity-0'"
        @load="imgLoaded = true"
        @error="imgError = true"
      >

      <!-- No logo fallback — gradient with initials badge, matching Free TV -->
      <div
        v-if="!hasLogo || imgError"
        class="relative flex size-full items-center justify-center overflow-hidden bg-gradient-to-br from-indigo-950 via-purple-900/70 to-slate-950"
      >
        <div class="absolute inset-0 bg-[radial-gradient(circle_at_center,rgba(255,255,255,0.08),transparent_70%)] pointer-events-none" />
        <div class="relative z-10 flex flex-col items-center justify-center text-center">
          <div class="flex size-11 items-center justify-center rounded-2xl bg-black/40 ring-1 ring-white/15">
            <span class="text-title-medium font-black tracking-widest text-white drop-shadow">
              {{ channelInitials }}
            </span>
          </div>
        </div>
      </div>

      <!-- Decorative overlay control: `tabindex="-1"` so the remote walks
           channels and not two stops per card. The favourite is reachable
           from the player and the channel row instead. -->
      <span
        class="absolute right-2 top-2 grid size-7 cursor-pointer place-items-center rounded-full bg-black/60 shadow transition-colors hover:bg-black/80 group-focus-visible:opacity-100 group-hover:opacity-100"
        :class="fav ? 'opacity-100' : 'opacity-0'"
        role="button"
        tabindex="-1"
        :aria-label="fav ? $t('Remove from favorites') : $t('Add to favorites')"
        @click.stop.prevent="emit('toggleFavorite', channel)"
      >
        <v-icon :icon="mdiStar" size="15" :class="fav ? 'text-amber-400' : 'text-white/70'" />
      </span>

      <div class="absolute inset-0 grid place-items-center opacity-0 transition-opacity duration-200 group-focus-visible:opacity-100 group-hover:opacity-100">
        <div class="grid size-11 place-items-center rounded-full bg-primary shadow-lg shadow-primary/40 ring-2 ring-white/20">
          <v-icon :icon="mdiPlay" size="22" class="ml-0.5 text-on-primary" />
        </div>
      </div>
    </div>

    <div class="flex flex-1 flex-col gap-1 px-3 py-2.5">
      <h3 class="line-clamp-1 text-body-medium font-semibold leading-snug" :title="displayName">
        {{ displayName }}
      </h3>

      <div class="flex items-center gap-1 text-label-small text-white/45">
        <span v-if="subtitle" class="line-clamp-1">{{ subtitle }}</span>
      </div>

      <template v-if="nowProgram">
        <p class="mt-0.5 line-clamp-1 text-label-small font-medium text-primary/80">
          {{ nowProgram.title }}
        </p>
        <div class="mt-1">
          <div class="h-0.5 w-full overflow-hidden rounded-full bg-white/8">
            <!-- scaleX, not width — see LiveChannelRow for why. -->
            <div
              class="h-full w-full origin-left rounded-full bg-primary/70 transition-transform duration-1000 ease-linear"
              :style="{ transform: `scaleX(${epgProgress / 100})` }"
            />
          </div>
        </div>
      </template>
    </div>
  </button>
</template>
