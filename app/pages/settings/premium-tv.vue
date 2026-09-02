<script setup lang="ts">
import {
  mdiCastConnected,
  mdiClockOutline,
  mdiLogout,
  mdiShieldCheckOutline,
  mdiShieldOffOutline,
} from '@mdi/js'
import { pushEntitlement, premiumApi } from '~/utils/premiumTv'

const settings = useSettingsStore()
const premium = usePremiumTvStore()

const DAY_MS = 24 * 60 * 60 * 1000
const TRIAL_DAYS = 30
/** "No expiry" as a date, because the gate's contract is a timestamp. */
const UNLIMITED_DAYS = 100 * 365
const statusLoaded = ref(false)

onMounted(async () => {
  // Quick health check — tells us whether the API server started at all.
  premiumApi.health()
    .then(() => console.log('[premium-tv] API server is up'))
    .catch((e: unknown) => console.error('[premium-tv] API server unreachable:', e))
  try {
    await premium.loadStatus()
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
const activationBusy = ref(false)
const activationMessage = ref('')
const activationError = ref('')

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
const disconnect = () => withBusy(() => premium.disconnect())

/**
 * Only the local flag is written here. The API keeps its own copy of the
 * entitlement — it is a separate process and re-checks it on every
 * request, the stream redirector included — and `app.vue` watches these
 * two fields and pushes the change there. One watcher rather than a call
 * beside every assignment, because a restored backup writes them too.
 */
function applyTier(tier: 'free' | 'premium', expiresAt: number): void {
  settings.subscriptionTier = tier
  settings.subscriptionExpiresAt = expiresAt
}

async function activate(tier: 'free' | 'premium', expiresAt: number): Promise<void> {
  activationBusy.value = true
  activationMessage.value = ''
  activationError.value = ''
  applyTier(tier, expiresAt)
  try {
    // Do not leave the setting page dependent on app.vue's background
    // watcher. The user needs the local API gate open before Connect can
    // test a provider, and this makes that transition immediate.
    await pushEntitlement(tier, expiresAt || null)
    activationMessage.value = tier === 'premium'
      ? $t('Premium TV is active. You can now connect your provider.')
      : $t('Premium TV has been deactivated.')
    // loadStatus populates the provider card but must not block
    // activation — the entitlement is already set via IPC.
    await premium.loadStatus().catch(() => {})
  }
  catch {
    activationError.value = $t('Could not activate Premium TV. Restart the app and try again.')
  }
  finally {
    activationBusy.value = false
  }
}

function activateTrial() {
  return activate('premium', Date.now() + TRIAL_DAYS * DAY_MS)
}

function activateUnlimited() {
  return activate('premium', Date.now() + UNLIMITED_DAYS * DAY_MS)
}

function deactivate() {
  return activate('free', 0)
}

function formatExpiry(ms: number): string {
  if (!ms)
    return ''
  return new Date(ms).toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  })
}
</script>

<template>
  <div class="flex flex-col gap-8">
    <!-- Subscription status -->
    <settings-section
      :title="$t('Subscription')"
      :hint="$t('Premium TV unlocks Xtream and M3U IPTV sources with EPG, favourites and more.')"
    >
      <v-card
        v-if="premium.connected"
        rounded="xl"
        class="panel flex flex-col items-start gap-3 p-6"
      >
        <div class="flex items-center gap-3">
          <v-icon :icon="mdiShieldCheckOutline" size="24" color="success" />
          <span class="text-title-medium font-bold text-success">{{ $t('Premium active') }}</span>
        </div>
        <p v-if="settings.subscriptionExpiresAt" class="text-body-small opacity-70">
          {{ $t('Expires {date}', { date: formatExpiry(settings.subscriptionExpiresAt) }) }}
        </p>
        <p v-else class="text-body-small opacity-70">
          {{ $t('No expiry date set.') }}
        </p>
        <v-btn
          variant="text"
          color="error"
          :prepend-icon="mdiLogout"
          size="small"
          :loading="activationBusy"
          @click="deactivate"
        >
          {{ $t('Deactivate') }}
        </v-btn>
      </v-card>

      <v-card
        v-else
        rounded="xl"
        class="panel flex flex-col items-start gap-3 p-6"
      >
        <div class="flex items-center gap-3">
          <v-icon :icon="mdiCastConnected" size="24" class="opacity-40" />
          <span class="text-title-medium">{{ $t('No active subscription') }}</span>
        </div>
        <p class="text-body-medium max-w-prose opacity-70">
          {{ $t('Activate a subscription to access Premium TV features.') }}
        </p>
        <div class="flex flex-wrap gap-2">
          <v-btn
            variant="tonal"
            color="primary"
            :prepend-icon="mdiClockOutline"
            :loading="activationBusy"
            @click="activateTrial"
          >
            {{ $t('Login via IPTV') }}
          </v-btn>
          <v-btn
            variant="tonal"
            color="secondary"
            :prepend-icon="mdiShieldCheckOutline"
            :loading="activationBusy"
            @click="activateUnlimited"
          >
            {{ $t('Activate unlimited') }}
          </v-btn>
        </div>
      </v-card>

      <v-alert
        v-if="activationMessage"
        type="success"
        variant="tonal"
        class="mt-3"
        :text="activationMessage"
      />
      <v-alert
        v-if="activationError"
        type="error"
        variant="tonal"
        class="mt-3"
        :text="activationError"
      />
    </settings-section>

    <!-- The provider itself: connected, or the one box that connects it -->
    <settings-section
      :title="$t('Provider')"
      :hint="$t('Paste the link your provider gave you. Only live channels are imported.')"
    >
      <div class="flex flex-col gap-4">
        <premium-tv-premium-account-card
          v-if="premium.connected && premium.account"
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

        <!-- The same form the Premium TV page mounts, without its heading:
             this section already has one. On success it opens Premium TV,
             which is where the channels are. -->
        <premium-tv-premium-connect-form v-else-if="settings.isPremium" :heading="false" />

        <v-alert
          v-else
          type="info"
          variant="tonal"
          :text="$t('Activate Premium TV above before connecting a provider.')"
        />

        <v-btn
          v-if="settings.isPremium"
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
  </div>
</template>
