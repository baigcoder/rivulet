<script setup lang="ts">
import type { ReleaseSort } from '~/utils/quality'
import type { MediaType } from '~/utils/tmdb'
import type { Release } from '~/utils/torrents'
import {
  mdiAccountGroup,
  mdiAlertCircleOutline,
  mdiCheck,
  mdiDownload,
  mdiFlashOutline,
  mdiFormatListBulletedType,
  mdiPlay,
  mdiPowerPlugOutline,
  mdiReload,
  mdiWeightLifter,
} from '@mdi/js'
import { sortReleases, tierLabel, tierOf, tiersIn } from '~/utils/quality'
import { ranked } from '~/utils/torrents'

/**
 * Every release the configured sources offer for a title, when the automatic
 * pick isn't what you want — a 4k copy, a specific release group, or just more
 * seeders. With no sources configured there is nothing to search, and the
 * dialog says so rather than showing an empty list.
 *
 * The dialog is a sibling of the Releases button, not a child: a `<button>`
 * wrapping Play/Download is invalid HTML, and those clicks never reach the player.
 */
const props = defineProps<{
  type: MediaType
  id: string | number
  imdbId?: string | null
  season?: number
  episode?: number
}>()

const downloads = useDownloadsStore()
const settings = useSettingsStore()

const open = ref(false)
const torrents = ref<Release[]>([])
const pending = ref(false)
const error = ref('')
const busy = ref('')
const step = ref('')
const added = ref<string[]>([])

const tier = ref('all')
const query = ref('')
const sort = ref<'best' | ReleaseSort>('best')

const SORTS = computed(() => [
  { value: 'best', title: $t('Best first') },
  { value: 'quality-desc', title: $t('Highest quality') },
  { value: 'quality-asc', title: $t('Lowest quality') },
  { value: 'size-desc', title: $t('Largest') },
  { value: 'size-asc', title: $t('Smallest') },
  { value: 'seeders', title: $t('Most seeders') },
] as const)

/** Nothing to search until the user has added somewhere to search. */
const configured = computed(() => settings.sources.length > 0)

async function load() {
  if (!configured.value) {
    error.value = NO_SOURCES()
    return
  }
  if (!props.imdbId) {
    error.value = $t('TMDB has no IMDb id for this title, so there is nothing to look it up with.')
    return
  }
  pending.value = true
  error.value = ''
  try {
    torrents.value = await findReleases(props.imdbId, props.season, props.episode)
  }
  catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
  finally {
    pending.value = false
  }
}

// Only search once the dialog is actually opened, and again if the episode changed.
watch(open, value => value && !torrents.value.length && load())
watch(() => [props.imdbId, props.season, props.episode].join('|'), () => {
  torrents.value = []
  added.value = []
})

/**
 * One chip per resolution the sources returned, read by `tierOf` — so "4k DV |
 * HDR", "UHD" and "2160p" are one chip, and a tier nobody has is never offered.
 */
const tiers = computed(() => {
  const count = (tier: string) => torrents.value.filter(t => tierOf(t) === tier).length
  const unknown = count('')
  return [
    { value: 'all', title: $t('All · {count}', { count: torrents.value.length }) },
    ...tiersIn(torrents.value).map(t => ({ value: t, title: `${tierLabel(t)} · ${count(t)}` })),
    ...unknown ? [{ value: 'unknown', title: $t('Unknown · {count}', { count: unknown }) }] : [],
  ]
})

// A new search may not carry the tier that was being looked at.
watch(tiers, list => {
  if (!list.some(t => t.value === tier.value))
    tier.value = 'all'
})

function shownQuality(t: Release) {
  return t.quality || tierLabel(tierOf(t))
}

// Same pick the Play button would make, storage budget included — picking by
// hand can still exceed it, and eviction will make room.
const best = computed(() => pickBest(torrents.value, downloads.budget))

/**
    * Too big for the drive to hold at all — a FAT32 stick stops at 4 GiB. Unlike
    * the budget, no amount of eviction makes room for one of these, so Play on a
    * torrent row is disabled rather than merely marked: the download would run
    * to the 4 GiB mark and die there.
    *
 * A direct link is exempt for Play: streaming it writes nothing. Download
 * is the engine, so it still honours the cap even when the same row has a URL.
 */
function tooBig(t: Release) {
  return !t.url && t.bytes > downloads.fileLimit
}

/** Only a magnet starts the torrent engine. A URL-only row is Play, not Download. */
function canSave(t: Release) {
  return !!t.magnet && !(t.bytes > downloads.fileLimit)
}

function inEngine(t: Release) {
  const hash = t.hash.toLowerCase()
  return hash && downloads.torrents.some(x => x.info_hash.toLowerCase() === hash)
}

function isAdded(t: Release) {
  return added.value.includes(releaseKey(t)) || inEngine(t)
}

/**
 * Every release, filtered and in the chosen order. "Best first" is Play's own
 * ranking, with what Play would never pick (dead, too big) after it rather than
 * hidden — this list is where you go to overrule that judgement.
 */
const list = computed(() => {
  const q = query.value.trim().toLowerCase()
  const shown = torrents.value.filter(t =>
    (tier.value === 'all' || (tier.value === 'unknown' ? !tierOf(t) : tierOf(t) === tier.value))
    && (!q || `${t.name} ${t.source} ${t.file ?? ''}`.toLowerCase().includes(q)))
  if (sort.value !== 'best')
    return sortReleases(shown, sort.value)
  const head = ranked(shown, downloads.budget, false, settings.allowTorrents)
  const picked = new Set(head)
  return [...head, ...sortReleases(shown.filter(t => !picked.has(t)), 'seeders')]
})

async function play(t: Release) {
  if (tooBig(t) || !(t.url || t.magnet))
    return
  // Follows How Play works, as the Play button does. With the engine on, a row
  // that carries a magnet streams through the torrent engine — downloading as it
  // plays — even when it has a direct link too; with it off, that link opens and
  // nothing touches the engine. A row with only one of the two plays that one
  // either way, and a torrent too big for the drive falls back to its link where
  // there is one. Stash the payload; debrid links blow past `?url=`.
  const viaEngine = !!t.magnet
    && (settings.allowTorrents || !t.url)
    && !(t.url && t.bytes > downloads.fileLimit)
  savePendingRelease(viaEngine ? { magnet: t.magnet } : { url: t.url })
  open.value = false
  await navigateTo(watchLink(
    props.type,
    props.id,
    props.season,
    props.episode,
    {
      pick: String(Date.now()),
      imdb: props.imdbId ? String(props.imdbId) : undefined,
    },
  ))
}

async function download(t: Release) {
  if (!canSave(t))
    return
  busy.value = releaseKey(t)
  step.value = ''
  error.value = ''
  try {
    const key = progressKey(props.type, props.id, props.season, props.episode)
    // Filed under the title, so this is the copy its Play button uses from now
    // on — picking a release by hand is a decision, not a one-off. The tick
    // waits until the engine lists the hash — metadata can take a minute.
    const started = await downloads.start(key, {
      magnet: t.magnet,
      fileIndex: t.fileIdx,
      season: props.season,
      episode: props.episode,
      allowTorrents: true,
      save: true,
      onStep: msg => { step.value = msg },
    })
    if (started.id < 0 || !started.hash)
      throw new Error($t('Torrent engine offline. Launch the native desktop or Android app to play torrents.'))
    if (!downloads.torrents.some(x => x.info_hash.toLowerCase() === started.hash.toLowerCase()))
      throw new Error($t('The torrent engine accepted the magnet but Downloads is still empty — wait a moment and try again, or restart the app.'))
    added.value = [...added.value, releaseKey(t)]
  }
  catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
  finally {
    busy.value = ''
    step.value = ''
  }
}

function saveHint(t: Release) {
  if (t.bytes > downloads.fileLimit)
    return $t('Too big for the download drive')
  if (isAdded(t))
    return $t('In downloads')
  if (!t.magnet)
    return $t('This release has no torrent to download — Play it instead.')
  return $t('Download')
}

defineExpose({
  open: () => { open.value = true },
})
</script>

<template>
  <v-btn :prepend-icon="mdiFormatListBulletedType" variant="tonal" :disabled="!imdbId" @click="open = true">
    {{ $t('Releases') }}
  </v-btn>

  <v-dialog v-model="open" max-width="1000" scrollable>
    <v-card rounded="xl">
      <v-card-title class="flex items-center gap-3 text-title-medium">
        <span>{{ $t('Pick a release') }}</span>
        <span v-if="torrents.length" class="text-body-small opacity-55">
          {{ $t('{shown} of {total}', { shown: list.length, total: torrents.length }) }}
        </span>
        <v-spacer />
        <v-btn icon size="small" variant="text" color="on-surface" :loading="pending" @click="load">
          <v-icon :icon="mdiReload" size="20" />
          <v-tooltip activator="parent" :text="$t('Search again')" />
        </v-btn>
      </v-card-title>

      <p v-if="step" class="px-4 pb-2 text-body-small text-primary">
        {{ step }}
      </p>

      <!-- Chips rather than a dropdown: a remote walks them left and right and
           OK picks one, where a select is one more overlay to open and close. -->
      <div class="flex flex-col gap-1 px-4 pb-3">
        <div class="flex flex-wrap items-center gap-x-3">
          <span class="w-16 shrink-0 text-label-large opacity-60">{{ $t('Quality') }}</span>
          <v-chip-group v-model="tier" mandatory selected-class="bg-primary text-on-primary font-medium" class="min-w-0 flex-1">
            <v-chip v-for="t in tiers" :key="t.value" :value="t.value" :text="t.title" size="small" />
          </v-chip-group>
        </div>
        <div class="flex flex-wrap items-center gap-x-3">
          <span class="w-16 shrink-0 text-label-large opacity-60">{{ $t('Sort') }}</span>
          <v-chip-group v-model="sort" mandatory selected-class="bg-secondary-container text-on-secondary-container font-medium" class="min-w-0 flex-1">
            <v-chip v-for="o in SORTS" :key="o.value" :value="o.value" :text="o.title" size="small" />
          </v-chip-group>
        </div>
        <search-field v-model="query" :placeholder="$t('Release or origin')" density="compact" class="sm:max-w-80" />
      </div>

      <v-card-text class="max-h-[60vh] pt-0">
        <div v-if="!configured" class="flex flex-col items-center gap-3 py-10 text-center">
          <v-icon :icon="mdiPowerPlugOutline" size="36" class="opacity-40" />
          <span class="max-w-md text-body-medium opacity-70">
            {{ $t('Rivulet has no sources configured, so there is nowhere to search. You can still play a magnet or a torrent file you open yourself.') }}
          </span>
          <v-btn variant="tonal" size="small" :to="localePath('/settings/sources')">
            {{ $t('Open source settings') }}
          </v-btn>
        </div>

        <div v-else-if="error" class="flex flex-col items-center gap-2 py-10">
          <v-icon :icon="mdiAlertCircleOutline" color="error" size="36" />
          <span class="text-body-medium opacity-70">{{ error }}</span>
        </div>

        <div v-else-if="pending && !torrents.length" class="flex flex-col items-center gap-2 py-10">
          <v-progress-circular indeterminate color="primary" />
          <span class="text-body-small opacity-55">{{ $t('Searching your sources…') }}</span>
        </div>

        <div v-else-if="!list.length" class="py-10 text-center text-body-medium opacity-60">
          {{ $t('Nothing matches.') }}
        </div>

        <div v-else class="flex flex-col gap-1">
          <!-- The columns add up to more than a phone is wide, so `basis-full`
                 gives the release name its own line there and the rest of the
                 numbers flow underneath it. -->
          <div
            v-for="t in list"
            :key="releaseKey(t)"
            class="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-xl px-2 py-2 hover:bg-surface-container-high/60 focus-within:bg-surface-container-high/60"
            :class="tooBig(t) ? 'opacity-45' : ''"
          >
            <div class="min-w-0 flex-1 basis-full sm:basis-0">
              <div class="flex items-center gap-2">
                <span class="truncate text-body-medium" :title="t.name">{{ t.name }}</span>
                <v-chip v-if="best && releaseKey(t) === releaseKey(best)" size="x-small" color="primary" :text="$t('Best')" class="shrink-0" />
              </div>
              <!-- Season packs name the episode here and nowhere else. -->
              <div v-if="t.file" class="truncate text-body-small opacity-50" :title="t.file">
                {{ t.file }}
              </div>
            </div>

            <v-chip size="x-small" :text="shownQuality(t) || $t('unknown')" class="shrink-0" />

            <!-- Amber is "costs more than it's worth", error is "will not fit
                   at all" — the second has to outrank the first, since a bloated
                   release is usually also the oversized one. -->
            <span
              class="shrink-0 text-body-small sm:w-20 sm:text-right"
              :class="tooBig(t) ? 'text-error' : isBloated(t) ? 'text-warning' : 'opacity-70'"
            >
              {{ t.size || bytesText(t.bytes) }}
              <v-tooltip
                v-if="tooBig(t)"
                activator="parent"
                :text="$t('Over the {limit} single-file limit of the drive downloads go to — reformat it in Settings → Storage to use releases this big', { limit: bytesText(downloads.fileLimit) })"
              />
              <v-tooltip v-else-if="isBloated(t)" activator="parent" :text="$t('Bigger than this quality needs — may not keep up while streaming')" />
            </span>

            <!-- Direct only when there is no torrent to file. A hash plus a
                 play URL is still a torrent — Download starts the engine. -->
            <span
              class="flex shrink-0 items-center gap-1 text-body-small sm:w-16 sm:justify-end"
              :class="t.url && !t.magnet ? 'text-primary' : 'opacity-70'"
            >
              <template v-if="t.url && !t.magnet">
                <v-icon :icon="mdiFlashOutline" size="13" />{{ $t('Direct') }}
                <v-tooltip activator="parent" :text="$t('The source fetched this already — it plays at once')" />
              </template>
              <template v-else>
                <v-icon :icon="mdiAccountGroup" size="13" />{{ t.seeders }}
              </template>
            </span>

            <span class="hidden w-24 shrink-0 truncate text-body-small opacity-50 sm:block">{{ t.source }}</span>

            <div class="ml-auto flex shrink-0 items-center sm:ml-0">
              <v-btn
                icon
                size="small"
                variant="text"
                color="on-surface"
                :disabled="tooBig(t) || !(t.url || t.magnet)"
                @click.stop="play(t)"
              >
                <v-icon :icon="mdiPlay" size="20" />
                <v-tooltip activator="parent" :text="tooBig(t) ? $t('Too big for the download drive') : $t('Play this source')" />
              </v-btn>
              <v-btn
                icon
                size="small"
                variant="text"
                color="on-surface"
                :disabled="!canSave(t)"
                :loading="busy === releaseKey(t)"
                @click.stop="download(t)"
              >
                <v-icon :icon="isAdded(t) ? mdiCheck : mdiDownload" size="20" />
                <v-tooltip activator="parent" :text="saveHint(t)" />
              </v-btn>
            </div>
          </div>
        </div>
      </v-card-text>

      <v-card-actions>
        <!-- One legend at a time, and a drive that cannot hold the release
               outranks a release that is merely fatter than it needs to be.
               Shown at every width, unlike the amber note: a dimmed row with no
               explanation reads as the app being broken. -->
        <span v-if="list.some(tooBig)" class="flex items-center gap-1 pl-2 text-body-small text-error">
          <v-icon :icon="mdiAlertCircleOutline" size="14" />
          {{ $t('Dimmed rows are over the {limit} file limit of the download drive.', { limit: bytesText(downloads.fileLimit) }) }}
        </span>
        <!-- The legend is the first thing to go when the row gets tight; the
               amber itself still carries a tooltip. -->
        <span v-else class="hidden items-center gap-1 pl-2 text-body-small opacity-45 sm:flex">
          <v-icon :icon="mdiWeightLifter" size="14" />
          {{ $t('Sizes in amber cost more bandwidth than the picture is worth.') }}
        </span>
        <v-spacer />
        <v-btn variant="text" size="small" :to="localePath('/downloads')">
          {{ $t('Downloads') }}
        </v-btn>
        <v-btn variant="text" size="small" @click="open = false">
          {{ $t('Close') }}
        </v-btn>
      </v-card-actions>
    </v-card>
  </v-dialog>
</template>
