<script setup lang="ts">
import {
  mdiCloudCheckOutline,
  mdiDeleteOutline,
  mdiInformationOutline,
  mdiMagnet,
  mdiPlus,
  mdiPowerPlugOutline,
} from '@mdi/js'
import { invoke } from '@tauri-apps/api/core'

/**
 * Where the app is allowed to look for something to play. Rivulet ships with
 * this list empty and never adds to it on its own: a source is a server
 * someone else runs, and adding one is the user's decision to make.
 */
const settings = useSettingsStore()
const ui = useUiStore()

const url = ref('')
const error = ref('')

function isDebridSource(src: string): boolean {
  return /torbox|realdebrid|premiumize|alldebrid|debridlink|offcloud|putio|easydebrid/i.test(src)
}

/** A new array rather than a push: localStorage and the search watcher see it. */
function append(value: string) {
  settings.sources = [...settings.sources, value]
}

function add(customUrl?: string) {
  const target = customUrl || url.value
  if (!target.trim())
    return

  // Takes whatever the user copied — a stremio:// link, a manifest URL, a bare
  // origin — and reduces it to the base the search appends to.
  const value = normalizeSource(target)
  if (!value) {
    error.value = $t('That doesn\'t look like a URL. Paste an addon link, or one starting with https://')
    return
  }
  if (settings.sources.includes(value)) {
    error.value = $t('That source is already in the list.')
    return
  }

  append(value)
  url.value = ''
  error.value = ''
}

function remove(value: string) {
  settings.sources = settings.sources.filter(s => s !== value)
}

// --- Deep links ---------------------------------------------------------------

/** A `rivulet://` link staged by plugins/deeplink.client.ts, awaiting a yes. */
const pending = computed(() => ui.pendingSource)

function confirmPending() {
  if (pending.value && !settings.sources.includes(pending.value))
    append(pending.value)
  ui.pendingSource = ''
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
      :title="$t('Playback source')"
      :hint="$t('Where playback comes from, and whether the torrent engine may be used at all.')"
    >
      <v-switch
        v-model="settings.allowTorrents"
        color="primary"
        density="comfortable"
        hide-details
        :label="settings.allowTorrents ? $t('Best available (Torrent Engine ON)') : $t('Direct streams only (Torrent Engine OFF)')"
      />

      <!-- Both states stay on screen: the trade-off is readable without
           flipping anything. The active one is bright, the other dimmed. -->
      <div class="mt-2 flex flex-col gap-1.5">
        <p
          class="flex items-start gap-2 text-body-small transition-opacity"
          :class="settings.allowTorrents ? 'opacity-95' : 'opacity-35'"
        >
          <v-icon
            :icon="settings.allowTorrents ? 'mdiCheckCircle' : 'mdiCircleOutline'"
            size="16"
            class="mt-0.5 shrink-0"
          />
          <span><strong>{{ $t('ON:') }}</strong> {{ $t('Torrent engine ENABLED — downloads P2P torrents to disk while watching, and plays direct server links when available.') }}</span>
        </p>
        <p
          class="flex items-start gap-2 text-body-small transition-opacity"
          :class="settings.allowTorrents ? 'opacity-35' : 'opacity-95'"
        >
          <v-icon
            :icon="settings.allowTorrents ? 'mdiCircleOutline' : 'mdiCheckCircle'"
            size="16"
            class="mt-0.5 shrink-0"
          />
          <span><strong>{{ $t('OFF:') }}</strong> {{ $t('Torrent engine DISABLED — direct server streams only (e.g. Debrid). P2P torrent downloading is turned off.') }}</span>
        </p>
      </div>
    </settings-section>

    <settings-section
      :title="$t('Sources')"
      :hint="$t('Rivulet searches nothing by itself. A source is a URL you add here, pointing at a server that answers with things to play. What a source offers, and whether you have the right to play it, is between you and whoever runs it.')"
    >
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

      <v-alert variant="tonal" border="start" border-color="primary" class="mt-3 text-body-small" rounded="lg">
        <strong>{{ $t('Torrentio Addon') }}</strong>:
        {{ $t('Provides torrent streams from scraped torrent providers (supports RealDebrid, Premiumize, AllDebrid, DebridLink, EasyDebrid, Offcloud, TorBox, and Put.io). Configure and get your link from') }}
        <a href="https://torrentio.strem.fun" target="_blank" class="text-primary font-medium underline">torrentio.strem.fun</a>
      </v-alert>

      <v-list v-if="settings.sources.length" bg-color="transparent" class="rounded-lg bg-surface-container/40">
        <v-list-item v-for="value in settings.sources" :key="value" :title="value">
          <template #subtitle>
            <div class="mt-1 flex items-center gap-2">
              <v-chip
                v-if="isDebridSource(value)"
                size="x-small"
                color="primary"
                variant="tonal"
                :prepend-icon="mdiCloudCheckOutline"
              >
                {{ $t('Direct / Debrid Stream') }}
              </v-chip>
              <v-chip
                v-else
                size="x-small"
                color="secondary"
                variant="tonal"
                :prepend-icon="mdiMagnet"
              >
                {{ $t('P2P Torrent Magnet') }}
              </v-chip>
            </div>
          </template>
          <template #append>
            <v-btn icon size="small" variant="text" color="on-surface" @click="remove(value)">
              <v-icon :icon="mdiDeleteOutline" size="20" />
              <v-tooltip activator="parent" :text="$t('Remove this source')" />
            </v-btn>
          </template>
        </v-list-item>
      </v-list>

      <div v-else class="flex flex-col items-center gap-2 rounded-lg bg-surface-container/40 px-4 py-10 text-center">
        <v-icon :icon="mdiPowerPlugOutline" size="32" class="opacity-40" />
        <p class="text-body-medium opacity-70">
          {{ $t('No sources yet. Until you add one, the app plays what you already have — anything in Downloads, and any magnet or torrent file you open yourself.') }}
        </p>
      </div>

      <v-card variant="outlined" rounded="lg" class="mt-2 border-outline-variant p-4">
        <div class="text-label-large font-bold mb-2 flex items-center gap-2">
          <v-icon :icon="mdiInformationOutline" size="18" color="primary" />
          <span>{{ $t('How Source URLs work with the Playback Source toggle:') }}</span>
        </div>
        <div class="grid grid-cols-1 gap-3 text-body-small sm:grid-cols-2">
          <div class="rounded-lg bg-primary/10 p-3">
            <div class="mb-1 flex items-center gap-1.5 font-semibold text-primary">
              <v-icon :icon="mdiCloudCheckOutline" size="16" />
              <span>{{ $t('Direct / Debrid Stream URL (Toggle ON)') }}</span>
            </div>
            <p class="opacity-80">
              {{ $t('Add a Debrid URL (e.g. TorBox, RealDebrid). Works when "Best available" toggle is ON for instant HTTPS server streaming with no disk downloads.') }}
            </p>
          </div>
          <div class="rounded-lg bg-secondary/10 p-3">
            <div class="mb-1 flex items-center gap-1.5 font-semibold text-secondary">
              <v-icon :icon="mdiMagnet" size="16" />
              <span>{{ $t('P2P Torrent Magnet URL (Toggle OFF)') }}</span>
            </div>
            <p class="opacity-80">
              {{ $t('Add a standard Torrent URL (e.g. torrentio.strem.fun/manifest.json). Works when toggle is OFF to download torrents to disk while streaming.') }}
            </p>
          </div>
        </div>
      </v-card>

      <div class="flex flex-col gap-2 mt-2">
        <div class="flex items-start gap-2">
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
            @update:model-value="error = ''"
          />
          <v-btn :prepend-icon="mdiPlus" variant="tonal" size="large" class="mt-1" @click="add()">
            {{ $t('Add') }}
          </v-btn>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <span class="text-body-small opacity-60">{{ $t('Quick Presets:') }}</span>
          <v-chip
            size="small"
            variant="tonal"
            color="primary"
            :prepend-icon="mdiCloudCheckOutline"
            @click="url = 'https://torrentio.strem.fun/torbox=YOUR_KEY/manifest.json'"
          >
            {{ $t('Debrid Stream (Toggle ON)') }}
          </v-chip>
          <v-chip
            size="small"
            variant="tonal"
            color="secondary"
            :prepend-icon="mdiMagnet"
            @click="add('https://torrentio.strem.fun/manifest.json')"
          >
            {{ $t('+ Add P2P Magnet Torrentio (Toggle OFF)') }}
          </v-chip>
        </div>
      </div>
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
