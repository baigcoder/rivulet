<script lang="ts" setup>
import type { MediaType } from '~/utils/tmdb'
import type { Release } from '~/utils/torrents'
import {
  mdiAccountGroup,
  mdiAlertCircleOutline,
  mdiArrowLeft,
  mdiDownload,
  mdiReload,
} from '@mdi/js'

// The player owns the whole window: no app bar, no drawer, no page scroll.
definePageMeta({ layout: false })

const route = useRoute()
const router = useRouter()
const downloads = useDownloadsStore()
const library = useLibraryStore()

const type = computed<MediaType>(() => route.query.type === 'tv' ? 'tv' : 'movie')
const id = computed(() => String(route.query.id ?? ''))
const season = computed(() => Number(route.query.s) || 0)
const episode = computed(() => Number(route.query.e) || 0)
// The downloads page knows exactly which file in a pack it wants played.
const fileIndex = computed(() => route.query.file == null ? null : Number(route.query.file))
const magnet = computed(() => String(route.query.magnet ?? ''))
/** A release the picker resolved to a plain link — played as-is, no engine. */
const link = computed(() => String(route.query.url ?? ''))

/** What this playback is remembered as — no id (a bare magnet) means nothing. */
const key = computed(() => id.value ? progressKey(type.value, id.value, season.value, episode.value) : '')

/** This title has a copy in the engine — one that may well need no network. */
const downloaded = computed(() => !!downloads.cachedFor(key.value))

// TMDB is only asked for the IMDb id (what a source is keyed by) and a title
// to show while the torrent warms up.
const { data: media, error: mediaError } = useMediaDetail(type, id)

// Offline, TMDB answers nothing — but anything played before left its poster and
// title in the local library, and that is enough to draw this page and to keep
// recording progress against.
const known = computed(() => library.media[titleKey(type.value, id.value)] ?? null)
const title = computed(() => media.value ?? known.value)

const step = ref($t('Loading title…'))
const errorMsg = ref('')
const torrent = ref<Release | null>(null)
const torrentId = ref<number | null>(null)
const src = ref('')

/**
 * The server streams the sources answered with, best first, and which one is
 * playing. Only direct-link playback has them: a torrent has no "other server"
 * to fail over to. The player lists them (server menu, quality menu) and asks
 * for a different index when one dies or you pick another copy.
 */
const candidates = ref<Release[]>([])
const activeCandidate = ref(0)

/** Stream-only mode found nothing to stream — a different message, and fix, than a plain failure. */
const noServerStream = ref(false)

const settings = useSettingsStore()

// The downloads store already polls every torrent's stats for the whole app, so
// a second poll of this one would only ask the engine the same question twice.
const stats = computed(() => downloads.torrents.find(t => t.id === torrentId.value)?.stats ?? null)

// Bumped on every start and on the way out, so a lookup that lands after you
// left the page — or jumped to another episode — doesn't reach back in and give
// the connection to something nobody is watching. (The trick useMediaFeed uses.)
let generation = 0

async function start() {
  const mine = ++generation
  errorMsg.value = ''
  noServerStream.value = false
  src.value = ''
  torrent.value = null
  candidates.value = []
  activeCandidate.value = 0

  try {
    // ?magnet=… hand-picks the release and skips the lookup — that's how the
    // downloads page replays something already in the engine, and the only
    // path that works with no sources configured.
    const started = await downloads.start(key.value, {
      // Waited for only if the sources are actually going to be searched. A copy
      // already on disk plays with TMDB unreachable, and hanging on this lookup
      // first is what used to make a downloaded film slow to start.
      imdbId: async () => {
        step.value = $t('Loading title…')
        await until(() => !!media.value || !!mediaError.value).toBe(true, { timeout: 20_000 })
        return media.value?.imdbId
      },
      // Read only once the lookup above has answered, so a download the app
      // never filed under this title can still be recognised by its name.
      named: () => title.value,
      magnet: magnet.value,
      url: link.value,
      season: season.value,
      episode: episode.value,
      fileIndex: fileIndex.value,
      allowTorrents: settings.allowTorrents,
      onStep: value => (step.value = value),
    })

    if (mine !== generation)
      return

    torrent.value = started.torrent
    // A direct link has no torrent behind it, so there are no stats to read.
    torrentId.value = started.url ? null : started.id

    // Pause everything else before the stream starts, so the first buffer gets
    // the whole connection. Nothing to pause for a finished torrent — see `focus`.
    await downloads.focus(started.id)

    step.value = $t('Buffering…')
    src.value = started.url || streamUrl(started.id, started.index)

    // Server playback carries the other answers with it; the player's menus and
    // the failover below walk this list.
    candidates.value = started.alternatives ?? []
    activeCandidate.value = 0
  }
  catch (e) {
    if (mine !== generation)
      return
    noServerStream.value = e instanceof NoServerStream
    errorMsg.value = e instanceof Error ? e.message : String(e)
  }
}

/**
 * The playing server died (or you picked another one from the menu): move down
 * the candidate list and remount the player on that URL. The `:key="src"` on
 * `<mpv-player>` makes the swap a fresh start, and progress already recorded by
 * the old mount is what the new one resumes from — so a film continues where it
 * stopped, on a different server.
 */
function useCandidate(index: number) {
  const next = candidates.value[index]
  if (!next || !next.url || index === activeCandidate.value)
    return
  activeCandidate.value = index
  torrent.value = next
  torrentId.value = null
  errorMsg.value = ''
  src.value = next.url
}

/** Playback of the current server failed — silently move to the next one, if any. */
function onPlaybackFailed() {
  if (!candidates.value.length)
    return
  const following = activeCandidate.value + 1
  if (following < candidates.value.length)
    useCandidate(following)
}

/** Host of a source base URL — "https://addon.example/manifest…" → "addon.example". */
function hostOf(via: string) {
  try {
    return new URL(via).host
  }
  catch {
    return via
  }
}

const RESOLUTION = /\b(2160p|4k|1080p|720p|480p)\b/i

/** What to call a candidate in the quality menu — its resolution where one is named. */
function qualityLabel(r: Release) {
  const q = r.quality.match(RESOLUTION) ?? r.name.match(RESOLUTION)
  return ((q?.[1] ?? r.quality) || '').toUpperCase() || $t('Unknown')
}

/**
 * What the player's two menus show. Servers list everything; qualities list the
 * first candidate per resolution, since five copies of 1080p are one choice.
 */
const candidateMenus = computed(() => {
  if (!candidates.value.length)
    return null
  const servers = candidates.value.map((r, index) => ({
    index,
    label: hostOf(r.via ?? '') || r.source,
    detail: [r.quality || qualityLabel(r), r.size].filter(Boolean).join(' · '),
  }))
  const seen = new Map<string, number>()
  const qualities: { index: number, label: string, detail?: string }[] = []
  for (const [index, r] of candidates.value.entries()) {
    const label = qualityLabel(r)
    if (!seen.has(label)) {
      seen.set(label, index)
      qualities.push({ index, label, detail: r.size })
    }
  }
  return { servers, qualities }
})

// Driven by the route alone — the title resolving is `start`'s business now, so
// that a downloaded film never waits on TMDB. Fires again if you jump straight
// to another episode without leaving the player.
watch(
  () => [key.value, magnet.value, link.value, fileIndex.value].join('|'),
  () => start(),
  { immediate: true },
)

// Leaving the player stops the download and hands the connection back to
// whatever was paused for it. Every exit route unmounts — Esc, Back, the browser
// history, switching to another title — so this is the one place it belongs.
onBeforeUnmount(() => {
  generation++
  downloads.release()
})

// A magnet, a link and a copy on disk all need no TMDB, so a failed lookup is
// only a failure to play when the sources were the plan.
const failure = computed(() => errorMsg.value
  || (mediaError.value && !magnet.value && !link.value && !downloaded.value
    ? $t('Couldn\'t load this title from TMDB.')
    : ''))

const heading = computed(() => {
  const name = title.value?.title ?? (route.query.title as string) ?? $t('Loading…')
  return season.value && episode.value ? `${name} · S${season.value}E${episode.value}` : name
})

const progressPct = computed(() => {
  const s = stats.value
  return s?.total_bytes ? Math.min(100, (s.progress_bytes / s.total_bytes) * 100) : 0
})

const speed = computed(() => stats.value?.live?.download_speed.human_readable ?? '—')
const peers = computed(() => stats.value?.live?.snapshot.peer_stats.live ?? 0)
/**
 * One line for the player's "buffering" notice, where there's no room for a
 * table. Empty while a direct link plays: there is no swarm to report on, and
 * "0 peers" reads as a fault rather than as "not applicable".
 */
const statusLine = computed(() =>
  stats.value ? `${speed.value} · ${peers.value} peers · ${progressPct.value.toFixed(0)}%` : '')

const backdrop = computed(() => backdropUrl(title.value?.backdrop, 'w1280'))

// What the end-of-playback screen offers. The show's season list carries an
// episode count per season, which is all the rollover needs.
const next = computed(() => {
  if (!media.value || !season.value || !episode.value)
    return null
  const target = nextEpisode(media.value.seasons, { season: season.value, episode: episode.value, watched: true })
  if (!target)
    return null
  return {
    to: watchLink('tv', id.value, target.season, target.episode),
    label: $t('Next · S{season} E{episode}', { season: target.season, episode: target.episode }),
  }
})

function leave() {
  if (router.options.history.state.back)
    router.back()
  else
    navigateTo(localePath('/'))
}

// preventDefault marks the press as used up, which is how the remote's back key
// knows it doesn't also have to go back a page (see plugins/dpad.client.ts).
useEventListener(window, 'keydown', (e: KeyboardEvent) => {
  if (e.key === 'Escape') {
    e.preventDefault()
    leave()
  }
})
</script>

<template>
  <v-app>
    <v-main class="h-dvh overflow-hidden bg-black text-white">
      <!-- Everything before mpv has a stream to open. -->
      <div v-if="!src" class="relative grid h-full place-items-center">
        <img
          v-if="backdrop"
          :src="backdrop"
          alt=""
          class="absolute inset-0 h-full w-full object-cover opacity-20 blur-2xl"
        >

        <div class="relative flex max-w-xl flex-col items-center gap-3 px-6 text-center">
          <template v-if="failure">
            <v-icon :icon="mdiAlertCircleOutline" color="error" size="40" />
            <div class="text-title-large">
              {{ $t('Nothing to play') }}
            </div>
            <p class="text-body-medium opacity-70">
              {{ failure }}
            </p>
            <!-- Stream-only mode's fix is one toggle away — right here, so a
                 title that only torrents can serve doesn't dead-end. -->
            <div v-if="noServerStream" class="mt-2 flex gap-2">
              <v-btn
                variant="tonal"
                :prepend-icon="mdiDownload"
                @click="settings.allowTorrents = true; start()"
              >
                {{ $t('Turn torrent streaming on') }}
              </v-btn>
              <v-btn variant="text" :prepend-icon="mdiArrowLeft" @click="leave">
                {{ $t('Back') }}
              </v-btn>
            </div>
            <div v-else class="mt-2 flex gap-2">
              <v-btn variant="tonal" :prepend-icon="mdiReload" @click="start">
                {{ $t('Try again') }}
              </v-btn>
              <v-btn variant="text" :prepend-icon="mdiArrowLeft" @click="leave">
                {{ $t('Back') }}
              </v-btn>
            </div>
          </template>

          <template v-else>
            <v-progress-circular indeterminate color="primary" size="40" />
            <div class="text-title-large">
              {{ heading }}
            </div>
            <div class="text-body-medium opacity-70">
              {{ step }}
            </div>
            <div v-if="torrent" class="text-body-small opacity-50">
              {{ torrent.quality }} · {{ torrent.size }}
              <template v-if="torrent.url">
                · {{ $t('direct link') }}
              </template>
              <template v-else>
                · {{ $t('{count} seeders', { count: torrent.seeders }) }}
              </template>
              · {{ torrent.source }}
              <div class="mt-1 truncate">
                {{ torrent.name }}
              </div>
            </div>
            <v-btn class="mt-2" variant="text" size="small" :prepend-icon="mdiArrowLeft" @click="leave">
              {{ $t('Back') }}
            </v-btn>
          </template>
        </div>
      </div>

      <!-- :key so picking a different file/torrent/server gets a fresh mpv process. -->
      <mpv-player
        v-else
        :key="src"
        :src="src"
        :status="statusLine"
        :media="title"
        :next="next"
        :imdb-id="media?.imdbId"
        :title="title?.title ?? String(route.query.title ?? '')"
        :year="title?.year"
        :season="season"
        :episode="episode"
        :candidates="candidateMenus"
        :active-candidate="activeCandidate"
        fullscreen
        @failed="onPlaybackFailed"
        @use-candidate="useCandidate"
      >
        <template #start>
          <v-btn icon variant="text" density="comfortable" :title="$t('Back (Esc)')" @click="leave">
            <v-icon :icon="mdiArrowLeft" />
          </v-btn>
        </template>

        <template #info>
          <div class="flex min-w-0 items-center gap-4">
            <div class="min-w-0">
              <div class="truncate text-title-medium">
                {{ heading }}
              </div>
              <div v-if="torrent" class="truncate text-body-small opacity-50">
                {{ torrent.quality }} · {{ torrent.size }} · {{ torrent.source }}
              </div>
            </div>

            <v-spacer />

            <!-- Swarm figures, so only while a torrent is what's playing. -->
            <div v-if="stats" class="flex shrink-0 items-center gap-3 text-body-small opacity-70">
              <span class="flex items-center gap-1" :title="$t('Download speed')">
                <v-icon :icon="mdiDownload" size="14" />{{ speed }}
              </span>
              <span class="flex items-center gap-1" :title="$t('Connected peers')">
                <v-icon :icon="mdiAccountGroup" size="14" />{{ peers }}
              </span>
              <span class="tabular-nums" :title="$t('Downloaded')">{{ progressPct.toFixed(0) }}%</span>
              <span class="hidden opacity-50 xl:inline">
                {{ bytesText(stats.progress_bytes) }} / {{ bytesText(stats.total_bytes) }}
              </span>
            </div>
          </div>
        </template>
      </mpv-player>
    </v-main>
  </v-app>
</template>
