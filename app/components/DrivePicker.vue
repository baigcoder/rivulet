<script setup lang="ts">
import { mdiCheckCircle, mdiHarddisk } from '@mdi/js'
import { invoke } from '@tauri-apps/api/core'

/**
 * A computer's drives as one button each, with their free space, and a warning
 * when the one in use is nearly full. Pressing one moves downloads — and Stream
 * only's cache with them — to a `Rivulet` folder on that drive. Used on the
 * Storage page and in the "play while downloading" choice, so the two can't
 * drift apart. Shows nothing where there are no drives to list (Android names
 * its own through the bridge, and a plain browser has no Rust to ask).
 */
const settings = useSettingsStore()
const downloads = useDownloadsStore()

interface Drive { name: string, path: string, free: number, total: number }
const drives = ref<Drive[]>([])
const error = ref('')

async function readDrives() {
  if (storageVolumes())
    return
  drives.value = await invoke<Drive[]>('list_drives').catch(() => [])
}
onMounted(readDrives)

/** The drive downloads land on now: the folder chosen, or the default one. */
const current = computed(() => {
  const dir = (settings.downloadDir || downloads.resolvedDir).toLowerCase()
  return drives.value
    .filter(d => dir.startsWith(d.path.toLowerCase()))
    .sort((a, b) => b.path.length - a.path.length)[0] ?? null
})

/** Under this much room a film may not fit, and a stream's cache may not either. */
const LOW_BYTES = 10 * 1024 ** 3
const lowSpace = computed(() => {
  const now = current.value
  return !!now && now.free < LOW_BYTES && drives.value.some(d => d.path !== now.path && d.free > now.free)
})

async function use(drive: Drive) {
  error.value = ''
  try {
    // Rust approves only a drive it listed, so this cannot name any folder.
    settings.downloadDir = await invoke<string>('folders_use_drive', { root: drive.path })
  }
  catch (e) {
    error.value = `${e}`
  }
  await readDrives()
}
</script>

<template>
  <div v-if="drives.length" class="flex flex-col gap-2">
    <v-alert
      v-if="lowSpace && current"
      type="warning"
      variant="tonal"
      density="compact"
      :text="$t('{drive} has only {free} free. Pick another drive below — downloads and Stream only both use the folder chosen here.', { drive: current.name, free: bytesText(current.free) })"
    />
    <p class="text-body-medium opacity-80">
      {{ $t('Drive') }}
    </p>
    <div class="flex flex-wrap gap-2">
      <v-btn
        v-for="drive in drives"
        :key="drive.path"
        :prepend-icon="current?.path === drive.path ? mdiCheckCircle : mdiHarddisk"
        :variant="current?.path === drive.path ? 'tonal' : 'outlined'"
        :color="drive.free < LOW_BYTES ? 'warning' : undefined"
        @click="use(drive)"
      >
        {{ $t('{drive} · {free} free', { drive: drive.name, free: bytesText(drive.free) }) }}
      </v-btn>
    </div>
    <p v-if="error" class="text-body-small text-error">
      {{ error }}
    </p>
  </div>
</template>
