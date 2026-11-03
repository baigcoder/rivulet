<script setup lang="ts">
/**
 * Source management for Live TV. After the Premium TV rewrite, the
 * only source that lives here is the built-in Free TV. Premium M3U
 * and Premium Xtream have moved to the `premium/` module and a
 * separate page under `/live-tv/premium`. The page stays as a thin
 * registry view so the existing routes and the "Manage sources"
 * link on the index page keep working.
 */
import type { GuideReport } from '~/utils/iptv'
import { mdiCheck, mdiChevronLeft, mdiClose, mdiPlus, mdiRefresh, mdiTelevision } from '@mdi/js'
import { guideUrls, importGuides, liveRemoveSource, liveSetActive, setGuideUrls } from '~/utils/iptv'
import { mapIptvError } from '~/utils/iptvErrors'

definePageMeta({ layout: 'default' })

const liveTv = useLiveTvStore()

const confirmRemove = ref<{ id: string, name: string } | null>(null)
const busy = ref(false)
const error = ref('')

async function load() {
  await liveTv.refreshSources()
}

onMounted(load)

async function setActive(id: string) {
  busy.value = true
  error.value = ''
  try {
    await liveSetActive(id)
    await load()
    await liveTv.loadDashboard()
    await liveTv.loadVisible({ reset: true })
  }
  catch (e) {
    error.value = mapIptvError(String(e)).key
  }
  finally {
    busy.value = false
  }
}

async function removeSource(id: string) {
  busy.value = true
  error.value = ''
  try {
    await liveRemoveSource(id)
    confirmRemove.value = null
    await load()
  }
  catch (e) {
    error.value = mapIptvError(String(e)).key
  }
  finally {
    busy.value = false
  }
}

function goBack() {
  navigateTo(localePath('/live-tv'))
}

// ── Programme guide ─────────────────────────────────────────────────
// Rivulet has no guide of its own; these are the user's XMLTV addresses.
const guides = ref<string[]>([])
const newGuide = ref('')
const guideBusy = ref(false)
const guideReport = ref<GuideReport | null>(null)
const guideError = ref('')

onMounted(async () => {
  guides.value = await guideUrls().catch(() => [])
})

async function saveGuides(next: string[]) {
  guideError.value = ''
  try {
    guides.value = await setGuideUrls(next)
  }
  catch (e) {
    guideError.value = String(e)
  }
}

async function addGuide() {
  const url = newGuide.value.trim()
  if (!url)
    return
  await saveGuides([...guides.value, url])
  if (guides.value.includes(url))
    newGuide.value = ''
  else
    guideError.value = $t('Only http and https addresses can be added.')
}

async function updateGuide() {
  guideBusy.value = true
  guideError.value = ''
  try {
    guideReport.value = await importGuides()
    liveTv.resetEpg()
  }
  catch (e) {
    guideError.value = String(e)
  }
  finally {
    guideBusy.value = false
  }
}

function kindLabel(kind: string): string {
  switch (kind) {
    case 'free-m3u': return $t('Free TV')
    default: return kind
  }
}

function statusLabel(status: string): string {
  switch (status) {
    case 'active': return $t('Active')
    case 'staging': return $t('Importing...')
    case 'failed': return $t('Failed')
    case 'superseded': return $t('Superseded')
    default: return status
  }
}
</script>

<template>
  <div class="flex h-full flex-col">
    <div class="flex items-center gap-3 px-4 pt-4 md:px-6">
      <v-btn icon variant="text" color="on-surface" @click="goBack">
        <v-icon :icon="mdiChevronLeft" />
      </v-btn>
      <h1 class="text-headline-medium font-bold">
        {{ $t('Live TV Sources') }}
      </h1>
    </div>

    <div class="px-4 pt-2 text-body-small opacity-60 md:px-6">
      {{ $t('The built-in Free TV source is always available. Premium providers live under Premium TV.') }}
    </div>

    <v-alert
      v-if="error"
      type="error"
      variant="tonal"
      class="mx-4 mt-4 md:mx-6"
      closable
      @click:close="error = ''"
    >
      {{ error }}
    </v-alert>

    <div class="min-h-0 flex-1 overflow-y-auto px-4 py-4 md:px-6">
      <div class="mx-auto flex max-w-3xl flex-col gap-3">
        <div
          v-for="source in liveTv.sources"
          :key="source.id"
          class="rounded-2xl border border-white/10 bg-surface-container-high p-4"
        >
          <div class="flex items-start gap-3">
            <v-icon
              :icon="mdiTelevision"
              size="32"
              :color="source.status === 'active' ? 'primary' : 'on-surface'"
              class="mt-1"
            />

            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <h2 class="truncate text-title-large font-semibold">
                  {{ source.displayName || kindLabel(source.kind) }}
                </h2>
                <span
                  v-if="source.status === 'active'"
                  class="inline-flex items-center gap-1 rounded-full bg-green-500/15 px-2 py-0.5 text-label-small font-medium text-green-400"
                >
                  <v-icon :icon="mdiCheck" size="12" />
                  {{ $t('Active') }}
                </span>
                <span
                  v-else
                  class="inline-flex items-center gap-1 rounded-full bg-white/10 px-2 py-0.5 text-label-small opacity-60"
                >
                  {{ statusLabel(source.status) }}
                </span>
              </div>

              <p class="mt-1 text-body-small opacity-60">
                {{ kindLabel(source.kind) }}
                <span v-if="source.id === 'free:iptv-org'">
                  · {{ $t('Built-in') }}
                </span>
              </p>

              <div class="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-body-small opacity-80">
                <span>
                  <strong>{{ source.channelCount.toLocaleString() }}</strong> {{ $t('channels') }}
                </span>
                <span v-if="source.countryCount > 0">
                  <strong>{{ source.countryCount }}</strong> {{ $t('countries') }}
                </span>
                <span v-if="source.categoryCount > 0">
                  <strong>{{ source.categoryCount }}</strong> {{ $t('categories') }}
                </span>
              </div>
            </div>
          </div>

          <div class="mt-4 flex flex-wrap gap-2">
            <v-btn
              v-if="source.status !== 'active'"
              color="primary"
              variant="flat"
              size="small"
              :loading="busy"
              @click="setActive(source.id)"
            >
              {{ $t('Activate') }}
            </v-btn>
          </div>
        </div>

        <section class="mt-4 rounded-2xl border border-white/10 bg-surface-container-high p-4" aria-labelledby="guide-heading">
          <h2 id="guide-heading" class="text-title-large font-semibold">
            {{ $t('Programme guide') }}
          </h2>
          <p class="mt-1 text-body-small opacity-70">
            {{ $t('Rivulet has no guide of its own. Add an XMLTV address (.xml or .xml.gz) from a guide you trust; channels are matched by their tvg-id.') }}
          </p>

          <ul v-if="guides.length" class="mt-3 flex list-none flex-col gap-1 ps-0">
            <li
              v-for="url in guides"
              :key="url"
              class="flex items-center gap-2 rounded-lg bg-surface-container px-3 py-1.5"
            >
              <span class="min-w-0 flex-1 truncate text-body-small" :title="url">{{ url }}</span>
              <button
                type="button"
                class="grid size-9 shrink-0 place-items-center rounded-lg opacity-70 transition-colors hover:bg-white/10 hover:opacity-100 focus-visible:bg-white/10 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                :aria-label="$t('Remove {name}', { name: url })"
                @click="saveGuides(guides.filter(g => g !== url))"
              >
                <v-icon :icon="mdiClose" size="18" />
              </button>
            </li>
          </ul>

          <form class="mt-3 flex gap-2" @submit.prevent="addGuide">
            <v-text-field
              v-model.trim="newGuide"
              :label="$t('Guide address')"
              variant="solo-filled"
              density="comfortable"
              rounded="lg"
              flat
              hide-details
              autocomplete="off"
              spellcheck="false"
              inputmode="url"
              class="min-w-0 flex-1"
            />
            <v-btn type="submit" color="primary" variant="tonal" :prepend-icon="mdiPlus" :disabled="!newGuide">
              {{ $t('Add') }}
            </v-btn>
          </form>

          <div class="mt-3 flex flex-wrap items-center gap-3">
            <v-btn
              variant="flat"
              color="primary"
              :prepend-icon="mdiRefresh"
              :loading="guideBusy"
              :disabled="!guides.length"
              @click="updateGuide"
            >
              {{ $t('Update guide now') }}
            </v-btn>
            <span v-if="guideReport" class="text-body-small opacity-80" role="status">
              {{ $t('{programs} programmes for {channels} channels', { programs: guideReport.programs.toLocaleString(), channels: guideReport.channels.toLocaleString() }) }}
            </span>
          </div>
          <p v-for="url in guideReport?.failed ?? []" :key="url" class="mt-2 text-body-small text-error">
            {{ $t('Could not read {url}', { url }) }}
          </p>
          <p v-if="guideError" class="mt-2 text-body-small text-error" role="alert">
            {{ guideError }}
          </p>
        </section>
      </div>
    </div>

    <v-dialog :model-value="!!confirmRemove" max-width="420" @update:model-value="confirmRemove = null">
      <v-card v-if="confirmRemove">
        <v-card-title>{{ $t('Remove source') }}</v-card-title>
        <v-card-text>
          {{ $t('Remove "{name}" and all {count} channels?', { name: confirmRemove?.name ?? '', count: liveTv.sources.find(s => s.id === confirmRemove?.id)?.channelCount ?? 0 }) }}
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn variant="text" @click="confirmRemove = null">
            {{ $t('Cancel') }}
          </v-btn>
          <v-btn color="error" variant="tonal" :loading="busy" @click="confirmRemove && removeSource(confirmRemove.id)">
            {{ $t('Remove') }}
          </v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>
  </div>
</template>
