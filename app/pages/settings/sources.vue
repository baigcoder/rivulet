<script setup lang="ts">
import {
  mdiAlertCircleOutline,
  mdiArrowDown,
  mdiArrowUp,
  mdiCloudCheckOutline,
  mdiDeleteOutline,
  mdiDownload,
  mdiInformationOutline,
  mdiMagnet,
  mdiPlus,
  mdiPowerPlugOutline,
  mdiRefresh,
} from '@mdi/js'
import { invoke } from '@tauri-apps/api/core'

/**
 * Where the app is allowed to look for something to play. Rivulet ships with
 * this list empty and never adds to it on its own: a source is a server
 * someone else runs, and adding one is the user's decision to make.
 */
const settings = useSettingsStore()
const ui = useUiStore()
const downloads = useDownloadsStore()

const url = ref('')
const error = ref('')
/** Adding asks the source for its manifest first; this covers the round trip. */
const checking = ref(false)
/** A source that could not be reached on Add, offered again as "Add anyway". */
const unreachable = ref('')
/**
 * The name of a source just added that names no cloud (debrid) service. Said
 * at once, while the choice is still in front of the user, rather than first
 * discovered as a question on the Play button.
 */
const noCloud = ref('')

type PlayMode = 'engine' | 'direct'

/** Same shape as MOTION_MODES: title is a function so the locale can change. */
const PLAY_MODES: { value: PlayMode, title: () => string, icon: string }[] = [
  { value: 'engine', title: () => $t('Download while watching'), icon: mdiMagnet },
  { value: 'direct', title: () => $t('Stream only'), icon: mdiCloudCheckOutline },
]

const playMode = computed<PlayMode>({
  get: () => settings.allowTorrents ? 'engine' : 'direct',
  set: value => { settings.allowTorrents = value === 'engine' },
})

/** A new array rather than a push: localStorage and the search watcher see it. */
function append(value: string) {
  settings.sources = [...settings.sources, value]
}

/**
 * Everything Add and a confirmed `rivulet://` link have in common: the manifest
 * is asked for before anything is added, so a typo, a catalog-only addon or a
 * paid source under *Torrents only* is turned away at the door rather than
 * failing silently at every search. Returns '' on success, or why not.
 */
async function admit(value: string, force = false): Promise<string> {
  if (settings.sources.includes(value))
    return $t('That source is already in the list.')
  if (settings.torrentOnly && isDebridSource(value))
    return $t('Torrents only is on, and this looks like a source that needs a paid account.')
  try {
    const manifest = await fetchManifest(value)
    if (settings.torrentOnly && isDebridSource(value, manifest))
      return $t('Torrents only is on, and this looks like a source that needs a paid account.')
    append(value)
    settings.patchSource(value, { name: manifest.name, logo: manifest.logo, ms: manifest.ms, error: '', at: Date.now(), types: manifest.types, idPrefixes: manifest.idPrefixes })
    if (!isDebridSource(value, manifest))
      noCloud.value = manifest.name || value
    return ''
  }
  catch (e) {
    if (force) {
      append(value)
      return ''
    }
    if (e instanceof NotASource && e.reason === 'unreachable')
      unreachable.value = value
    return e instanceof Error ? e.message : String(e)
  }
}

async function add(force = false) {
  const target = force ? unreachable.value : url.value
  if (!target.trim())
    return

  // Takes whatever the user copied — a stremio:// link, a manifest URL, a bare
  // origin — and reduces it to the base the search appends to.
  const value = normalizeSource(target)
  if (!value) {
    error.value = $t('That doesn\'t look like a URL. Paste an addon link, or one starting with https://')
    return
  }
  checking.value = true
  unreachable.value = ''
  try {
    error.value = await admit(value, force)
  }
  finally {
    checking.value = false
  }
  if (error.value)
    return
  url.value = ''
  // Some addons encode *excluded* qualities in the path. Checking both
  // "3D" and "Non 3D" excludes every stream — the server answers 200 with [].
  if (/qualityfilter=[^/]*\bthreed\b/i.test(value) && /qualityfilter=[^/]*\bnonthreed\b/i.test(value))
    error.value = $t('This source excludes both 3D and non-3D releases, so it will never return anything. Reconfigure it and leave one of those filters off.')
}

function remove(value: string) {
  settings.sources = settings.sources.filter(s => s !== value)
  const { [value]: _gone, ...rest } = settings.sourceMeta
  settings.sourceMeta = rest
}

/** Order is preference: when two sources return the same release, the higher one's copy is kept. */
function move(value: string, by: -1 | 1) {
  const list = [...settings.sources]
  const at = list.indexOf(value)
  const to = at + by
  if (at < 0 || to < 0 || to >= list.length)
    return
  const [moved] = list.splice(at, 1)
  list.splice(to, 0, moved!)
  settings.sources = list
}

function setEnabled(value: string, on: boolean | null) {
  settings.patchSource(value, { disabled: !on })
}

/** Re-ask every source for its manifest: is it up, how fast, and what it calls itself now. */
const checkingAll = ref(false)
async function checkAll() {
  checkingAll.value = true
  try {
    await Promise.all(settings.sources.map(async value => {
      try {
        const m = await fetchManifest(value)
        // Reached it: a source sitting out after failures is back from now.
        resumeSource(value)
        settings.patchSource(value, { name: m.name, logo: m.logo, ms: m.ms, error: '', at: Date.now(), types: m.types, idPrefixes: m.idPrefixes, pausedUntil: 0 })
      }
      catch (e) {
        settings.patchSource(value, { error: e instanceof Error ? e.message : String(e), at: Date.now() })
      }
    }))
  }
  finally {
    checkingAll.value = false
  }
}

/** What kind of source this is, from what it has answered with — or a guess from its address. */
function kindOf(value: string): 'torrent' | 'direct' | 'both' | 'unknown' {
  return settings.sourceMeta[value]?.kind ?? (isDebridSource(value) ? 'direct' : 'unknown')
}

/** Sitting out after failing in a row: when it will be asked again, as a clock time. */
function pausedText(value: string) {
  const until = settings.sourceMeta[value]?.pausedUntil ?? 0
  if (until <= Date.now())
    return ''
  return new Date(until).toLocaleTimeString(settings.locale || undefined, { hour: '2-digit', minute: '2-digit' })
}

/** Under *Torrents only* a link-only source still answers, but nothing it says is used. */
function skipped(value: string) {
  return settings.torrentOnly && kindOf(value) === 'direct'
}

const kindLabel = computed(() => ({
  torrent: $t('Torrents'),
  direct: $t('Instant (cloud)'),
  both: $t('Torrents and instant (cloud)'),
  unknown: $t('Kind not known yet'),
}))

const relative = computed(() => new Intl.RelativeTimeFormat(settings.locale || undefined, { numeric: 'auto' }))

function ago(at?: number) {
  if (!at)
    return ''
  const minutes = Math.round((at - Date.now()) / 60_000)
  if (Math.abs(minutes) < 60)
    return relative.value.format(minutes, 'minute')
  const hours = Math.round(minutes / 60)
  if (Math.abs(hours) < 48)
    return relative.value.format(hours, 'hour')
  return relative.value.format(Math.round(hours / 24), 'day')
}

/** The health line under a source: its last error, or how fast and how much. */
function health(value: string) {
  const m = settings.sourceMeta[value]
  if (!m?.at)
    return $t('Not checked yet')
  if (m.error)
    return `${m.error} · ${ago(m.at)}`
  const parts = [$t('{ms} ms', { ms: m.ms ?? 0 })]
  if (m.count != null)
    parts.push($t('{count} results', { count: m.count }))
  parts.push(ago(m.at))
  return parts.join(' · ')
}

// --- Deep links ---------------------------------------------------------------

/** A `rivulet://` link staged by plugins/deeplink.client.ts, awaiting a yes. */
const pending = computed(() => ui.pendingSource)

async function confirmPending() {
  const value = pending.value
  ui.pendingSource = ''
  if (!value)
    return
  checking.value = true
  try {
    error.value = await admit(value)
  }
  finally {
    checking.value = false
  }
}

// `stremio://` is the scheme addon pages already publish, so handling it makes
// their existing install buttons work here. Off by default and never touched
// silently: a machine with Stremio installed has its own handler, and quietly
// taking the scheme would break it.
const stremioLinks = ref(false)
const stremioBusy = ref(false)

onMounted(async () => {
  try {
    stremioLinks.value = await useTauriDeepLinkIsRegistered('stremio')
  }
  catch {}
})

async function toggleStremio(on: boolean | null) {
  stremioBusy.value = true
  try {
    await (on ? useTauriDeepLinkRegister('stremio') : useTauriDeepLinkUnregister('stremio'))
    // Registering rewrites the .desktop entry, quotes and all, which is what
    // stops Chromium browsers from opening the link. Startup does this too.
    await invoke('deep_link_fix_handler').catch(() => {})
  }
  catch (e) {
    // macOS reads schemes from the bundle and cannot change them at runtime.
    error.value = e instanceof Error ? e.message : String(e)
    stremioLinks.value = !on
  }
  finally {
    stremioBusy.value = false
  }
}
</script>

<template>
  <div class="flex flex-col gap-8">
    <settings-section
      :title="$t('Your sources')"
      :hint="$t('Rivulet searches nothing by itself. A source is a URL you add here, pointing at a server that answers with things to play. What a source offers, and whether you have the right to play it, is between you and whoever runs it.')"
    >
      <v-list v-if="settings.sources.length" bg-color="transparent" class="rounded-lg bg-surface-container/40">
        <v-list-item
          v-for="(value, i) in settings.sources"
          :key="value"
          :class="{ 'opacity-60': settings.sourceMeta[value]?.disabled }"
        >
          <template #prepend>
            <v-avatar size="36" rounded="lg" color="surface-container-high" class="mr-1">
              <v-img v-if="settings.sourceMeta[value]?.logo" :src="settings.sourceMeta[value]?.logo" alt="" />
              <v-icon v-else :icon="mdiPowerPlugOutline" size="20" />
            </v-avatar>
          </template>
          <v-list-item-title class="text-body-large">
            {{ settings.sourceMeta[value]?.name || sourceHost(value) }}
          </v-list-item-title>
          <v-list-item-subtitle class="mt-1 flex flex-col gap-1">
            <span class="truncate text-body-small opacity-70">{{ value }}</span>
            <span class="flex flex-wrap items-center gap-2">
              <v-chip
                size="x-small"
                variant="tonal"
                :color="kindOf(value) === 'torrent' ? 'secondary' : 'primary'"
                :prepend-icon="kindOf(value) === 'torrent' ? mdiMagnet : mdiCloudCheckOutline"
              >
                {{ kindLabel[kindOf(value)] }}
              </v-chip>
              <v-chip v-if="skipped(value)" size="x-small" variant="tonal" color="warning">
                {{ $t('Skipped by Torrents only') }}
              </v-chip>
              <v-chip v-if="pausedText(value)" size="x-small" variant="tonal" color="warning">
                {{ $t('Failing — paused until {time}', { time: pausedText(value) }) }}
              </v-chip>
              <span
                class="text-body-small"
                :class="settings.sourceMeta[value]?.error ? 'text-error' : 'opacity-70'"
              >
                <v-icon v-if="settings.sourceMeta[value]?.error" :icon="mdiAlertCircleOutline" size="14" />
                {{ health(value) }}
              </span>
            </span>
          </v-list-item-subtitle>
          <template #append>
            <div class="flex items-center gap-1">
              <v-btn icon size="small" variant="text" color="on-surface" :disabled="i === 0" :aria-label="$t('Move up')" @click="move(value, -1)">
                <v-icon :icon="mdiArrowUp" size="20" />
                <v-tooltip activator="parent" :text="$t('Move up')" />
              </v-btn>
              <v-btn icon size="small" variant="text" color="on-surface" :disabled="i === settings.sources.length - 1" :aria-label="$t('Move down')" @click="move(value, 1)">
                <v-icon :icon="mdiArrowDown" size="20" />
                <v-tooltip activator="parent" :text="$t('Move down')" />
              </v-btn>
              <v-switch
                :model-value="!settings.sourceMeta[value]?.disabled"
                color="primary"
                density="compact"
                hide-details
                inset
                class="mx-1"
                :aria-label="$t('Use this source')"
                @update:model-value="setEnabled(value, $event)"
              />
              <v-btn icon size="small" variant="text" color="on-surface" :aria-label="$t('Remove this source')" @click="remove(value)">
                <v-icon :icon="mdiDeleteOutline" size="20" />
                <v-tooltip activator="parent" :text="$t('Remove this source')" />
              </v-btn>
            </div>
          </template>
        </v-list-item>
      </v-list>

      <div v-else class="flex flex-col items-center gap-2 rounded-lg bg-surface-container/40 px-4 py-10 text-center">
        <v-icon :icon="mdiPowerPlugOutline" size="32" class="opacity-40" />
        <p class="text-body-medium opacity-70">
          {{ $t('No sources yet. Until you add one, the app plays what you already have — anything in Downloads, and any magnet or torrent file you open yourself.') }}
        </p>
      </div>

      <div v-if="settings.sources.length" class="flex">
        <v-btn :prepend-icon="mdiRefresh" variant="text" size="small" :loading="checkingAll" @click="checkAll">
          {{ $t('Check all sources') }}
        </v-btn>
      </div>

      <div class="mt-2 flex items-start gap-2">
        <v-text-field
          v-model="url"
          :label="$t('Source URL')"
          placeholder="https://… or stremio://…"
          variant="solo-filled"
          density="comfortable"
          rounded="lg"
          flat
          :error-messages="error"
          @keydown.enter="add()"
          @update:model-value="error = ''; unreachable = ''"
        />
        <v-btn :prepend-icon="mdiPlus" variant="tonal" size="large" class="mt-1" :loading="checking" @click="add()">
          {{ $t('Add') }}
        </v-btn>
      </div>
      <div v-if="unreachable" class="flex">
        <v-btn variant="text" size="small" color="primary" @click="add(true)">
          {{ $t('Add it anyway') }}
        </v-btn>
      </div>
      <!-- The how-to under the list rather than over it: the list and the
           box are what this page is for, and on a TV the explanation pushed
           both off the first screen. -->
      <v-expansion-panels variant="accordion" bg-color="transparent">
        <v-expansion-panel :title="$t('How to add a source')">
          <v-expansion-panel-text>
            <div class="flex flex-col gap-3">
              <!-- Naming the protocol is what makes the empty box answerable: a user who
                 knows the word can find an addon in one search. Naming a particular
                 addon would make this project the one distributing the link. -->
              <v-alert variant="tonal" density="comfortable" rounded="lg" class="text-body-medium">
                <template #prepend>
                  <v-icon :icon="mdiInformationOutline" />
                </template>
                <!-- i18n-t rather than $t: the sentence has markup inside it, and the
                   two <code> spans are literal syntax a translator must not touch. -->
                <i18n-t keypath="Rivulet speaks the {protocol}. Any Stremio addon URL works here — paste the {link} link or the {manifest} address and it'll be trimmed to what's needed." tag="span">
                  <template #protocol>
                    <strong>{{ $t('Stremio addon protocol') }}</strong>
                  </template>
                  <template #link>
                    <code>stremio://</code>
                  </template>
                  <template #manifest>
                    <code>manifest.json</code>
                  </template>
                </i18n-t>
              </v-alert>
              <p class="text-body-medium opacity-70">
                {{ $t('Paste a manifest URL you found yourself — nothing is added until you press Add, and the address is checked first. Higher in the list wins when two sources return the same release.') }}
              </p>
            </div>
          </v-expansion-panel-text>
        </v-expansion-panel>
      </v-expansion-panels>
    </settings-section>

    <settings-section
      :title="$t('How Play works')"
      :hint="$t('This is only the Play button. Download and picking a release from the list still work either way.')"
    >
      <settings-segment v-model="playMode" :options="PLAY_MODES" />

      <!-- One mode at a time: the inactive trade-off is noise at ten feet. -->
      <div class="rounded-xl bg-surface-container/40 px-4 py-3">
        <p class="text-title-small">
          {{ settings.allowTorrents ? $t('Play downloads while you watch') : $t('Play streams and keeps nothing') }}
        </p>
        <ul v-if="settings.allowTorrents" class="mt-2 list-disc space-y-2 pl-5 text-body-medium opacity-90">
          <li>{{ $t('Playback starts as soon as the first pieces arrive — you do not wait for the whole file.') }}</li>
          <li>{{ $t('The rest keeps downloading in the background while you watch, and stays in Downloads.') }}</li>
          <li>{{ $t('Works with any source that answers with torrents — free ones included.') }}</li>
        </ul>
        <ul v-else class="mt-2 list-disc space-y-2 pl-5 text-body-medium opacity-90">
          <li>{{ $t('A Direct link from your sources plays as it is. Nothing is added to the torrent engine.') }}</li>
          <li>{{ $t('With only torrents to go on, the torrent streams from a private cache inside the app — never your Downloads folder, never listed as a download — and is deleted when you leave the player.') }}</li>
          <li>{{ $t('So a free torrent source streams here too; a paid one is never required.') }}</li>
        </ul>
      </div>
      <!-- Stream only is instant only with a cloud source; say which one this is. -->
      <template v-if="!settings.allowTorrents && settings.sources.length">
        <v-alert
          :type="settings.hasInstantSource ? 'success' : 'info'"
          variant="tonal"
          density="compact"
          :text="settings.hasInstantSource
            ? $t('A cloud (debrid) source is on, so Play starts instantly whenever it has the title.')
            : $t('None of your sources is a cloud (debrid) one, so nothing can start instantly. Play asks first whether to download while you watch or stream from a temporary cache.')"
        />
        <v-switch
          v-if="!settings.hasInstantSource"
          v-model="settings.askBeforeTorrentStream"
          color="primary"
          density="comfortable"
          hide-details
          :label="$t('Ask before playing from a torrent')"
        />
      </template>
      <!-- Only with the engine on: a Direct link downloads nothing to keep going. -->
      <template v-if="settings.allowTorrents">
        <v-switch
          v-model="settings.keepDownloading"
          color="primary"
          density="comfortable"
          hide-details
          :label="$t('Keep downloading after leaving the player')"
        />
        <p class="text-body-medium opacity-70">
          {{ settings.keepDownloading
            ? $t('On: a film keeps downloading after you leave the player, until it finishes or you pause it in Downloads.')
            : $t('Off: a film downloads only while you watch. Leaving the player pauses it, and playing it again resumes where it stopped.') }}
        </p>
      </template>

      <v-switch
        v-model="settings.torrentOnly"
        color="primary"
        density="comfortable"
        hide-details
        :label="$t('Torrents only — free sources, no paid accounts')"
      />
      <p class="text-body-medium opacity-70">
        {{ settings.torrentOnly
          ? $t('On: links from debrid and other paid services are ignored, and sources that need a paid account cannot be added. Everything plays through the torrent engine.')
          : $t('Off: a source may answer with torrents, Direct links, or both, and either can play.') }}
      </p>
      <p class="text-body-medium opacity-70">
        {{ $t('Download still files a magnet through the engine, or saves a Direct link to disk. Releases still lets you play or save one specific row.') }}
      </p>
    </settings-section>

    <settings-section
      :title="$t('Adding by link')"
      :hint="$t('A page can offer a rivulet:// link that opens the app with a source ready to add. The app always asks first — a link can never change what Rivulet searches on its own.')"
    >
      <v-switch
        v-model="stremioLinks"
        :loading="stremioBusy"
        color="primary"
        density="comfortable"
        hide-details
        :label="$t('Also handle stremio:// links')"
        @update:model-value="toggleStremio"
      />
      <p class="text-body-small opacity-70">
        <i18n-t keypath="Addon pages publish {link} install links. Turning this on points them at Rivulet. Leave it off if you also use Stremio — only one app can own the scheme, and this would take it." tag="span">
          <template #link>
            <code>stremio://</code>
          </template>
        </i18n-t>
      </p>
    </settings-section>

    <!-- A link arrived. Show what it is, in full, before anything is added. -->
    <v-dialog :model-value="!!pending" max-width="560" persistent>
      <v-card rounded="xl">
        <v-card-title class="text-title-medium">
          {{ $t('Add this source?') }}
        </v-card-title>
        <v-card-text class="flex flex-col gap-3">
          <p class="text-body-medium">
            {{ $t('A link asked Rivulet to start searching:') }}
          </p>
          <code class="break-all rounded-lg bg-surface-container-high px-3 py-2 text-body-small">{{ pending }}</code>
          <p class="text-body-small opacity-70">
            {{ $t('Rivulet will send it the title you\'re looking for and play what it hands back. Only add servers you trust — this one is not run by, or checked by, this app.') }}
          </p>
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn variant="text" @click="ui.pendingSource = ''">
            {{ $t('Cancel') }}
          </v-btn>
          <v-btn variant="tonal" color="primary" @click="confirmPending">
            {{ $t('Add source') }}
          </v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <!-- Just added, and nothing about it says cloud: it can't start a film
         instantly, so suggest the mode that suits a torrent source. -->
    <v-dialog
      :model-value="!!noCloud"
      max-width="600"
      @update:model-value="open => !open && (noCloud = '')"
    >
      <v-card rounded="xl" class="pa-2">
        <v-card-title class="text-title-large font-bold">
          {{ $t('This source has no cloud service') }}
        </v-card-title>
        <v-card-text class="flex flex-col gap-4">
          <p class="text-body-medium opacity-85">
            {{ $t('{name} answers with torrents, not with a cloud (debrid) link, so a film cannot start instantly from it. It plays as the torrent arrives.', { name: noCloud }) }}
          </p>
          <template v-if="!settings.allowTorrents">
            <p class="text-body-medium opacity-85">
              {{ $t('Play while downloading suits a source like this best: playback starts as the first pieces arrive, and the whole file is kept in Downloads, so it never has to be fetched twice.') }}
            </p>
            <p v-if="settings.downloadDir || downloads.resolvedDir" class="break-all text-body-small opacity-70">
              {{ $t('Saving to: {folder}', { folder: settings.downloadDir || downloads.resolvedDir }) }}
            </p>
            <drive-picker />
          </template>
          <p v-else class="text-body-medium opacity-85">
            {{ $t('Play already downloads while you watch, so this source works as it is.') }}
          </p>
        </v-card-text>
        <v-card-actions class="flex-wrap gap-2">
          <v-spacer />
          <template v-if="!settings.allowTorrents">
            <v-btn variant="text" @click="noCloud = ''">
              {{ $t('Keep Stream only') }}
            </v-btn>
            <v-btn :prepend-icon="mdiDownload" variant="flat" color="primary" @click="settings.allowTorrents = true; noCloud = ''">
              {{ $t('Use Play while downloading') }}
            </v-btn>
          </template>
          <v-btn v-else variant="tonal" color="primary" @click="noCloud = ''">
            {{ $t('OK') }}
          </v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>

    <settings-section
      :title="$t('What a source has to speak')"
      :hint="$t('Any server implementing the Stremio addon protocol works — it is an open protocol with several independent implementations, and Rivulet runs no code from a source, only reads its answer.')"
    >
      <p class="text-body-small opacity-70">
        <i18n-t keypath="Rivulet asks a source for {movie}, or {series}, and expects a {streams} array back. Add several and their results are merged, with duplicates dropped and earlier sources preferred." tag="span">
          <template #movie>
            <code>/stream/movie/&lt;imdb-id&gt;.json</code>
          </template>
          <template #series>
            <code>/stream/series/&lt;imdb-id&gt;:&lt;season&gt;:&lt;episode&gt;.json</code>
          </template>
          <template #streams>
            <code>streams</code>
          </template>
        </i18n-t>
      </p>
      <p class="text-body-small opacity-70">
        {{ $t('The project does not host, run, endorse or recommend any source, and does not distribute a list of them.') }}
      </p>
    </settings-section>
  </div>
</template>
