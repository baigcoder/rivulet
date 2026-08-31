<script setup lang="ts">
/**
 * Premium TV connect form.
 *
 * The first tab is one box, because one box is what a provider actually
 * sells: a playlist link with the account's credentials in its query.
 * The Rust side recognises one of those as a *panel* and connects to its
 * API rather than downloading the playlist — that is what makes an
 * account status, an expiry date and a live-only channel list possible
 * from the same string — and falls back to importing it as a playlist if
 * the panel does not answer. The shape it looks for is written down in
 * `premium/xtream.rs`, deliberately not here: nothing in the browser
 * builds a provider URL, so nothing in the browser needs to know one.
 *
 * The manual tab stays for a provider that hands out three fields
 * instead of a link.
 *
 * The heading is a prop because this form is mounted in two places: on
 * its own page, where it needs one, and inside a *Settings* section that
 * already has one.
 *
 * This form is the only place a password is typed. It is held in a `ref`,
 * never logged, never written to localStorage, and gone the moment the
 * request returns; the Rust side encrypts it before the row hits SQLite.
 */
import { mdiCheckCircle, mdiLinkVariant, mdiServerNetwork } from '@mdi/js'
import { computed, onUnmounted, watch } from 'vue'

const { heading = true } = defineProps<{ heading?: boolean }>()

const premium = usePremiumTvStore()
const router = useRouter()

const mode = ref<'link' | 'manual'>('link')
const link = ref('')
const accountName = ref('')
const serverUrl = ref('')
const username = ref('')
const password = ref('')
const busy = ref(false)
const error = ref('')
const elapsedSeconds = ref(0)
let elapsedTimer: ReturnType<typeof setInterval> | null = null

/**
 * A few providers supply the panel API URL rather than three separate
 * fields. Accept it in either tab and use its first URL when a messaging app
 * has pasted the linked address twice.
 */
function cleanProviderLink(value: string): string {
  const trimmed = value.trim()
  // Some support chats copy a Markdown link rather than its URL. Keep the
  // target — it is the address that carries the account query parameters.
  const markdownTarget = trimmed.match(/^\[[^\]]*\]\((https?:\/\/[^\s)]+)\)$/i)?.[1]
  const candidate = markdownTarget ?? trimmed
  const secondUrl = /https?:\/\//gi
  secondUrl.exec(candidate)
  const second = secondUrl.exec(candidate)
  return second ? candidate.slice(0, second.index).trim() : candidate
}

function credentialsFromPanelUrl(value: string): { serverUrl: string, username: string, password: string } | null {
  try {
    const url = new URL(cleanProviderLink(value))
    if (!/\/(?:player_api|playlist)\.php$|\/get\.php$/i.test(url.pathname))
      return null
    const username = url.searchParams.get('username')?.trim()
    const password = url.searchParams.get('password')
    if (!username || !password)
      return null
    const directory = url.pathname.replace(/\/[^/]+$/, '').replace(/\/$/, '')
    return { serverUrl: `${url.origin}${directory}`, username, password }
  }
  catch {
    return null
  }
}

watch(serverUrl, value => {
  const credentials = credentialsFromPanelUrl(value)
  if (!credentials)
    return
  serverUrl.value = credentials.serverUrl
  username.value = credentials.username
  password.value = credentials.password
})

const progressMessage = computed(() => {
  if (elapsedSeconds.value < 6)
    return $t('Checking your provider…')
  if (elapsedSeconds.value < 20)
    return $t('Downloading the live channel list…')
  return $t('Importing a large live channel list…')
})

const progressDetail = computed(() => $t(
  'This can take a little while for providers with thousands of channels. {seconds}s elapsed.',
  { seconds: elapsedSeconds.value },
))

function beginProgress(): void {
  elapsedSeconds.value = 0
  if (elapsedTimer)
    clearInterval(elapsedTimer)
  elapsedTimer = setInterval(() => {
    elapsedSeconds.value++
  }, 1000)
}

function endProgress(): void {
  if (elapsedTimer)
    clearInterval(elapsedTimer)
  elapsedTimer = null
}

onUnmounted(endProgress)

async function submit() {
  busy.value = true
  error.value = ''
  beginProgress()
  try {
    if (mode.value === 'link') {
      if (!link.value.trim()) {
        error.value = $t('Paste the link your provider gave you.')
        return
      }
      await premium.connectM3u(cleanProviderLink(link.value), accountName.value.trim() || undefined)
    }
    else {
      if (!serverUrl.value || !username.value || !password.value) {
        error.value = $t('Server URL, username and password are all required.')
        return
      }
      await premium.connectXtream(
        serverUrl.value.trim(),
        username.value.trim(),
        password.value,
      )
    }
    await router.push(localePath('/live-tv/premium'))
  }
  catch {
    // The store has already turned this into a sentence a user can read —
    // a raw `e.message` here would be a `fetch` failure string at best and
    // an upstream error carrying the provider's URL at worst.
    error.value = premium.error || $t('That provider could not be reached.')
  }
  finally {
    endProgress()
    busy.value = false
  }
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <h1 v-if="heading" class="text-headline-medium font-bold">
      {{ $t('Connect a Premium TV provider') }}
    </h1>

    <v-card rounded="xl" class="panel pa-6">
      <v-tabs v-model="mode" color="primary" align-tabs="center">
        <v-tab value="link">
          <v-icon :icon="mdiLinkVariant" class="me-2" />
          {{ $t('Provider link') }}
        </v-tab>
        <v-tab value="manual">
          <v-icon :icon="mdiServerNetwork" class="me-2" />
          {{ $t('Server details') }}
        </v-tab>
      </v-tabs>

      <v-window v-model="mode" class="mt-6">
        <v-window-item value="link">
          <v-form @submit.prevent="submit">
            <v-text-field
              v-model="link"
              :label="$t('Provider link')"
              :hint="$t('An Xtream Codes link or a playlist URL. Your account is checked and only live channels are imported.')"
              persistent-hint
              placeholder="http://example.com:8080/…"
              variant="outlined"
              required
              autocomplete="off"
              spellcheck="false"
            />
            <v-text-field
              v-model="accountName"
              :label="$t('Display name (optional)')"
              variant="outlined"
              class="mt-4"
            />
            <v-btn
              type="submit"
              color="primary"
              :disabled="busy"
              :prepend-icon="mdiCheckCircle"
              class="mt-4"
            >
              <v-progress-circular v-if="busy" indeterminate size="16" width="2" class="me-2" />
              {{ busy ? progressMessage : $t('Connect') }}
            </v-btn>
          </v-form>
        </v-window-item>

        <v-window-item value="manual">
          <v-form @submit.prevent="submit">
            <v-text-field
              v-model="serverUrl"
              :label="$t('Server URL')"
              placeholder="http://example.com:8080"
              variant="outlined"
              required
              autocomplete="off"
            />
            <v-text-field
              v-model="username"
              :label="$t('Username')"
              variant="outlined"
              required
              autocomplete="off"
            />
            <v-text-field
              v-model="password"
              :label="$t('Password')"
              type="password"
              variant="outlined"
              required
              autocomplete="new-password"
            />
            <v-btn
              type="submit"
              color="primary"
              :disabled="busy"
              :prepend-icon="mdiCheckCircle"
              class="mt-4"
            >
              <v-progress-circular v-if="busy" indeterminate size="16" width="2" class="me-2" />
              {{ busy ? progressMessage : $t('Connect') }}
            </v-btn>
          </v-form>
        </v-window-item>
      </v-window>

      <v-alert
        v-if="busy"
        type="info"
        variant="tonal"
        class="mt-4"
        :title="progressMessage"
        :text="progressDetail"
      />

      <v-alert
        v-if="error"
        type="error"
        variant="tonal"
        class="mt-4"
        :text="error"
      />
    </v-card>
  </div>
</template>
