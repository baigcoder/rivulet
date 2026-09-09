<script setup lang="ts">
import type { MediaType } from '~/utils/tmdb'
import { mdiCheck, mdiDownload } from '@mdi/js'

// Same lookup the player does, minus the playing: the torrent stays in the
// engine and the downloads page takes it from there.
//
// `type`/`id` are what the download is filed under, so that pressing Play on
// this title later finds the copy this button fetched instead of searching for
// another one.
//
// The playback-source toggle only gates *Play*. Download always files a magnet
// in the torrent engine — a Direct URL keeps nothing on this device.
const props = defineProps<{
  type?: MediaType
  id?: string | number
  imdbId?: string | null
  season?: number
  episode?: number
  // Declared rather than left to fall through, so it reads the same way as the
  // Releases button it always stands next to and `check:types` sees the caller.
  size?: string
}>()

const downloads = useDownloadsStore()

const key = computed(() => props.type && props.id
  ? progressKey(props.type, props.id, props.season, props.episode)
  : '')

const state = ref<'idle' | 'busy' | 'done'>('idle')
const error = ref('')

// Filed under this title and still in the engine — coming back to the
// page must not look like Download again, or a second start 400s
// "already live" and the button turns into Retry.
const held = computed(() => {
  const filed = key.value ? downloads.cachedFor(key.value) : null
  if (!filed?.hash)
    return false
  const want = canonHash(filed.hash)
  return downloads.torrents.some(t => canonHash(t.info_hash) === want)
})

const done = computed(() => state.value === 'done' || held.value)

// Picking another episode makes the previous "In downloads" a lie.
watch(() => [props.imdbId, props.season, props.episode].join('|'), () => {
  state.value = 'idle'
  error.value = ''
})

async function download() {
  state.value = 'busy'
  error.value = ''
  try {
    const started = await downloads.start(key.value, {
      imdbId: props.imdbId,
      season: props.season,
      episode: props.episode,
      // How Play works only gates Play. Download always picks the best
      // magnet — Direct mode must still leave a copy on disk.
      allowTorrents: true,
      save: true,
    })
    if (started.id < 0 || !started.hash)
      throw new Error($t('Nothing here is a download — these sources only stream this title.'))
    state.value = 'done'
  }
  catch (e) {
    // Stay on this button. Opening Releases here made Download look
    // like a picker — Releases is its own control.
    error.value = e instanceof Error ? e.message : String(e)
    state.value = 'idle'
  }
}
</script>

<template>
  <v-btn
    :prepend-icon="done ? mdiCheck : mdiDownload"
    :loading="state === 'busy'"
    :color="done || !error ? undefined : 'error'"
    :to="done ? localePath('/downloads') : undefined"
    :disabled="!imdbId"
    :size="size"
    variant="tonal"
    @click="!done && download()"
  >
    {{ done ? $t('In downloads') : error ? $t('Retry download') : $t('Download') }}
    <v-tooltip v-if="error && !done" activator="parent" :text="error" />
  </v-btn>
</template>
