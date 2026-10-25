<script setup lang="ts">
import {
  mdiCheck,
  mdiClockOutline,
  mdiInfinity,
  mdiLogout,
  mdiShieldCheckOutline,
  mdiShieldOffOutline,
  mdiTelevisionPlay,
} from '@mdi/js'
import { premiumApi, pushEntitlement } from '~/utils/premiumTv'

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

/**
 * Where the user is in getting Premium TV going, as three steps. The page used
 * to be two unrelated sections, and nothing said that the second one needed
 * the first — or that, once both were done, the channels were somewhere else.
 */
const steps = computed(() => [
  { title: $t('Turn on Premium TV'), done: settings.isPremium },
  { title: $t('Connect your provider'), done: settings.isPremium && premium.connected },
  { title: $t('Watch'), done: false },
])
const currentStep = computed(() => steps.value.findIndex(s => !s.done))

/** Whole days left on the subscription, for the bar under it. */
const daysLeft = computed(() => settings.subscriptionExpiresAt
  ? Math.max(0, Math.ceil((settings.subscriptionExpiresAt - Date.now()) / DAY_MS))
  : 0)
const unlimited = computed(() => daysLeft.value > 365 * 10)

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
    <!-- Where you are: the two sections below depend on each other, and the
         end of it — the channels — is on another page. -->
    <ol class="m-0 grid list-none gap-3 p-0 sm:grid-cols-3" :aria-label="$t('Getting started')">
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
          v-if="i === 2 && currentStep === 2"
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

    <!-- Subscription status -->
    <settings-section
      :title="$t('Subscription')"
      :hint="$t('Premium TV unlocks Xtream and M3U IPTV sources with EPG, favourites and more.')"
    >
      <!-- The subscription, not the provider. `premium.connected` means an
           account is loaded, so this card called an active subscription with
           no provider yet "No active subscription" — directly above the banner
           saying Premium TV was active. -->
      <v-card
        v-if="settings.isPremium"
        rounded="xl"
        class="panel flex flex-col items-start gap-3 p-6"
      >
        <div class="flex items-center gap-3">
          <v-icon :icon="mdiShieldCheckOutline" size="24" color="success" />
          <span class="text-title-medium font-bold text-success">{{ $t('Subscription active') }}</span>
        </div>
        <p v-if="unlimited" class="flex items-center gap-1.5 text-body-medium opacity-80">
          <v-icon :icon="mdiInfinity" size="18" /> {{ $t('Unlimited — no expiry') }}
        </p>
        <template v-else-if="settings.subscriptionExpiresAt">
          <p class="text-body-medium opacity-80">
            {{ $t('{days} days left · expires {date}', { days: daysLeft, date: formatExpiry(settings.subscriptionExpiresAt) }) }}
          </p>
          <div class="h-1.5 w-full max-w-sm overflow-hidden rounded-full bg-white/10">
            <div class="h-full rounded-full bg-success" :style="{ width: `${Math.min(100, (daysLeft / TRIAL_DAYS) * 100)}%` }" />
          </div>
        </template>
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

      <!-- Two choices, each saying what it is. The first button used to read
           "Login via IPTV" and started a thirty-day trial — nothing was logged
           into, and the IPTV login is the section below. -->
      <div v-else class="grid gap-3 sm:grid-cols-2">
        <button
          type="button"
          class="flex flex-col items-start gap-2 rounded-2xl border border-white/10 bg-surface-container/60 p-5 text-start outline-none transition-colors hover:border-primary/60 focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50"
          :disabled="activationBusy"
          @click="activateTrial"
        >
          <v-icon :icon="mdiClockOutline" size="28" class="text-primary" />
          <span class="text-title-medium font-bold">{{ $t('Start a 30-day trial') }}</span>
          <span class="text-body-medium opacity-70">{{ $t('Everything in Premium TV for thirty days. Turn it off any time.') }}</span>
        </button>
        <button
          type="button"
          class="flex flex-col items-start gap-2 rounded-2xl border border-white/10 bg-surface-container/60 p-5 text-start outline-none transition-colors hover:border-primary/60 focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50"
          :disabled="activationBusy"
          @click="activateUnlimited"
        >
          <v-icon :icon="mdiInfinity" size="28" class="text-primary" />
          <span class="text-title-medium font-bold">{{ $t('Activate unlimited') }}</span>
          <span class="text-body-medium opacity-70">{{ $t('No expiry date. For your own provider, on this device.') }}</span>
        </button>
      </div>

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
      :hint="$t('Paste the link your provider gave you, or enter its server details. Xtream accounts bring movies and series too.')"
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
