<script setup lang="ts">
import type { ConnectionSummary } from '~/types/premium'
import { mdiAccountPlus, mdiCheck, mdiShieldOffOutline, mdiTelevisionPlay } from '@mdi/js'
import { premiumApi } from '~/utils/premiumTv'

const settings = useSettingsStore()
const premium = usePremiumTvStore()

const statusLoaded = ref(false)

onMounted(async () => {
  // Quick health check — tells us whether the API server started at all.
  premiumApi.health()
    .then(() => console.log('[premium-tv] API server is up'))
    .catch((e: unknown) => console.error('[premium-tv] API server unreachable:', e))
  try {
    await Promise.all([premium.loadStatus(), premium.loadConnections()])
  }
  finally {
    // This marks only the initial status lookup. A provider connection also
    // uses `connection = loading`; treating both states alike unmounts the
    // form midway through submit and loses the fields and error message.
    statusLoaded.value = true
  }
})

/** Guards the account card's own two buttons while either is in flight. */
const busy = ref(false)

async function withBusy(fn: () => Promise<unknown>): Promise<void> {
  busy.value = true
  try {
    await fn()
  }
  finally {
    busy.value = false
  }
}

const refresh = () => withBusy(() => premium.refresh(true))
// Signing out of the active account hands over to the next saved one,
// rather than leaving the others stranded behind the sign-in form.
function disconnect() {
  return withBusy(() => {
    const active = premium.connections.find(c => c.active)
    return active ? premium.removeConnection(active.id) : premium.disconnect()
  })
}

const others = computed(() => premium.connections.filter(c => !c.active))
const switchTo = (id: string) => withBusy(() => premium.switchTo(id))

/** The sign-in form under a connected account, for a second login. */
const adding = ref(false)
function addAnother(): void {
  adding.value = true
}

const removing = ref<ConnectionSummary | null>(null)
async function confirmRemove(): Promise<void> {
  const target = removing.value
  removing.value = null
  if (target)
    await withBusy(() => premium.removeConnection(target.id))
}

function accountKind(c: ConnectionSummary): string {
  return c.providerType === 'm3u' ? $t('M3U / M3U+') : $t('Xtream Codes')
}

/**
 * Where the user is in getting Premium TV going. It needs one thing: their own
 * paid IPTV account. Nothing is free for a while, and nothing is switched on apart — the
 * local switch that used to be step one said "active" to someone signed in to
 * nothing.
 */
const steps = computed(() => [
  { title: $t('Sign in with your provider'), done: premium.connected },
  { title: $t('Watch'), done: false },
])
const currentStep = computed(() => steps.value.findIndex(s => !s.done))
</script>

<template>
  <div class="flex flex-col gap-8">
    <!-- Where you are: the two sections below depend on each other, and the
         end of it — the channels — is on another page. -->
    <ol class="m-0 grid list-none gap-3 p-0 sm:grid-cols-2" :aria-label="$t('Getting started')">
      <li
        v-for="(step, i) in steps"
        :key="step.title"
        class="flex items-center gap-3 rounded-2xl border px-4 py-3"
        :class="step.done
          ? 'border-emerald-400/30 bg-emerald-400/8'
          : i === currentStep ? 'border-primary/50 bg-primary/10' : 'border-white/8 bg-surface-container/40 opacity-60'"
        :aria-current="i === currentStep ? 'step' : undefined"
      >
        <span
          class="grid size-8 shrink-0 place-items-center rounded-full text-label-large font-bold"
          :class="step.done ? 'bg-emerald-400 text-black' : i === currentStep ? 'bg-primary text-on-primary' : 'bg-white/10'"
        >
          <v-icon v-if="step.done" :icon="mdiCheck" size="18" />
          <template v-else>{{ i + 1 }}</template>
        </span>
        <span class="min-w-0 truncate text-title-small font-semibold">{{ step.title }}</span>
        <v-btn
          v-if="i === 1 && currentStep === 1"
          size="small"
          color="primary"
          variant="flat"
          class="ms-auto"
          :prepend-icon="mdiTelevisionPlay"
          :to="localePath('/live-tv/premium')"
        >
          {{ $t('Open') }}
        </v-btn>
      </li>
    </ol>

    <!-- The provider itself: connected, or the one box that connects it -->
    <settings-section
      :title="$t('Provider')"
      :hint="$t('Paste the link your provider gave you, or enter its server details. Xtream accounts bring movies and series too.')"
    >
      <div class="flex flex-col gap-4">
        <premium-tv-premium-account-card
          v-if="premium.account"
          :account="premium.account"
          :catalog="premium.catalog"
          :busy="busy"
          @refresh="refresh"
          @disconnect="disconnect"
        />

        <!-- Keep the connect form alive once the first status lookup has
             completed. A provider connection also changes `connection` to
             loading, and replacing this component clears its fields. -->
        <div
          v-else-if="!statusLoaded"
          class="grid place-items-center py-8"
        >
          <v-progress-circular indeterminate color="primary" size="32" />
        </div>

        <!-- Saved logins other than the active one. Switching drops the
             stream of the old one: a provider counts it as a connection. -->
        <ul v-if="statusLoaded && others.length" class="m-0 flex list-none flex-col gap-2 p-0" :aria-label="$t('Other accounts')">
          <li
            v-for="c in others"
            :key="c.id"
            class="flex flex-wrap items-center gap-3 rounded-2xl border border-white/8 bg-surface-container/40 px-4 py-3"
          >
            <div class="min-w-0 flex-1">
              <p class="truncate text-body-medium font-medium">
                {{ c.accountName || c.displayName }}
              </p>
              <p class="text-body-small opacity-60">
                {{ accountKind(c) }}
              </p>
            </div>
            <v-btn size="small" variant="tonal" color="primary" :disabled="busy" @click="switchTo(c.id)">
              {{ $t('Switch') }}
            </v-btn>
            <v-btn size="small" variant="text" :disabled="busy" @click="removing = c">
              {{ $t('Remove') }}
            </v-btn>
          </li>
        </ul>

        <!-- The same form the Premium TV page mounts, without its heading:
             this section already has one. On success it opens Premium TV,
             which is where the channels are. -->
        <premium-tv-premium-connect-form v-if="statusLoaded && (!premium.connected || adding)" :heading="false" />
        <v-btn
          v-else-if="premium.connected"
          variant="tonal"
          size="small"
          class="self-start"
          :prepend-icon="mdiAccountPlus"
          @click="addAnother"
        >
          {{ $t('Add another account') }}
        </v-btn>

        <v-btn
          v-if="premium.connected"
          variant="text"
          color="primary"
          :to="localePath('/live-tv/premium')"
          size="small"
          class="self-start"
        >
          {{ $t('Open Premium TV') }}
        </v-btn>
      </div>
    </settings-section>

    <!-- Content filtering -->
    <settings-section
      v-if="premium.connected"
      :title="$t('Content')"
      :hint="$t('Filter channels from your provider by content type.')"
    >
      <v-card rounded="xl" class="panel p-6">
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-3">
            <v-icon :icon="mdiShieldOffOutline" size="24" class="opacity-60" />
            <div>
              <p class="text-body-medium font-medium">
                {{ $t('Hide adult channels') }}
              </p>
              <p class="text-body-small opacity-60">
                {{ $t('Channels marked as 18+ by the provider or detected by category name will be hidden.') }}
              </p>
            </div>
          </div>
          <v-switch
            v-model="settings.hideAdultChannels"
            color="primary"
            density="compact"
            hide-details
            :aria-label="$t('Hide adult channels')"
          />
        </div>
      </v-card>
    </settings-section>

    <v-dialog
      :model-value="!!removing"
      max-width="480"
      @update:model-value="open => !open && (removing = null)"
    >
      <v-card rounded="xl">
        <v-card-title class="text-title-medium">
          {{ $t('Remove this account?') }}
        </v-card-title>
        <v-card-text class="text-body-medium">
          {{ $t('{name} and its saved login will be deleted from this device.', { name: removing?.accountName || removing?.displayName || '' }) }}
        </v-card-text>
        <v-card-actions>
          <v-spacer />
          <v-btn variant="text" @click="removing = null">
            {{ $t('Cancel') }}
          </v-btn>
          <v-btn variant="tonal" color="error" @click="confirmRemove">
            {{ $t('Remove') }}
          </v-btn>
        </v-card-actions>
      </v-card>
    </v-dialog>
  </div>
</template>
