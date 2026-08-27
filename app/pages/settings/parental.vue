<script setup lang="ts">
const settings = useSettingsStore()

const RATINGS = ['G', 'PG', 'PG-13', 'R', 'NC-17'] as const

const pinInput = ref('')
const pinConfirm = ref('')
const pinError = ref('')
const showPinChange = ref(false)

function setPin() {
  pinError.value = ''
  if (pinInput.value.length < 4) {
    pinError.value = $t('PIN must be at least 4 digits')
    return
  }
  if (pinInput.value !== pinConfirm.value) {
    pinError.value = $t('PINs do not match')
    return
  }
  settings.parentalPin = pinInput.value
  showPinChange.value = false
  pinInput.value = ''
  pinConfirm.value = ''
}

function clearPin() {
  settings.parentalPin = ''
}
</script>

<template>
  <div class="flex flex-col gap-8">
    <settings-section
      :title="$t('Parental controls')"
      :hint="$t('Restrict content by age rating. A PIN prevents changes without authorization.')"
    >
      <v-switch
        v-model="settings.parentalEnabled"
        color="primary"
        density="comfortable"
        hide-details
        :label="$t('Enable parental controls')"
      />

      <div v-if="settings.parentalEnabled" class="mt-4">
        <div class="text-label-medium mb-2 opacity-70">
          {{ $t('Maximum allowed rating') }}
        </div>
        <div class="flex flex-wrap gap-2">
          <v-chip
            v-for="r in RATINGS"
            :key="r"
            :variant="settings.parentalMaxRating === r ? 'flat' : 'outlined'"
            :color="settings.parentalMaxRating === r ? 'primary' : undefined"
            @click="settings.parentalMaxRating = r"
          >
            {{ r }}
          </v-chip>
        </div>
        <p class="text-body-small opacity-70 mt-2">
          {{ $t('Titles rated higher than {rating} will be hidden from browse pages and require a PIN to play.', { rating: settings.parentalMaxRating }) }}
        </p>
      </div>
    </settings-section>

    <settings-section
      :title="$t('PIN protection')"
      :hint="$t('Require a PIN to change parental controls or play restricted content.')"
    >
      <div v-if="settings.parentalPin" class="flex items-center gap-2">
        <span class="text-body-medium opacity-70">{{ $t('A PIN is set.') }}</span>
        <v-btn variant="text" size="small" @click="clearPin">
          {{ $t('Remove PIN') }}
        </v-btn>
      </div>
      <div v-else>
        <v-btn v-if="!showPinChange" variant="tonal" @click="showPinChange = true">
          {{ $t('Set a PIN') }}
        </v-btn>
        <div v-else class="flex flex-col gap-2">
          <v-text-field
            v-model="pinInput"
            type="password"
            :label="$t('PIN')"
            maxlength="8"
            density="comfortable"
            hide-details
            autofocus
          />
          <v-text-field
            v-model="pinConfirm"
            type="password"
            :label="$t('Confirm PIN')"
            maxlength="8"
            density="comfortable"
            hide-details
          />
          <div v-if="pinError" class="text-body-small text-error">
            {{ pinError }}
          </div>
          <div class="flex gap-2">
            <v-btn variant="text" size="small" @click="showPinChange = false">
              {{ $t('Cancel') }}
            </v-btn>
            <v-btn variant="tonal" size="small" @click="setPin">
              {{ $t('Save') }}
            </v-btn>
          </div>
        </div>
      </div>
    </settings-section>
  </div>
</template>
