<script setup lang="ts">
/**
 * Source management for Live TV. After the Premium TV rewrite, the
 * only source that lives here is the built-in Free TV. Premium M3U
 * and Premium Xtream have moved to the `premium/` module and a
 * separate page under `/live-tv/premium`. The page stays as a thin
 * registry view so the existing routes and the "Manage sources"
 * link on the index page keep working.
 */
import { mdiCheck, mdiChevronLeft, mdiTelevision } from '@mdi/js'
import { liveRemoveSource, liveSetActive } from '~/utils/iptv'
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
