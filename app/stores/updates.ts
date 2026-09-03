import type { Update } from '~/utils/updates'
import { invoke } from '@tauri-apps/api/core'
import { key } from '~/brand'
import { androidDismissUpdateNotification, androidDownloadUpdate, androidGetUpdateProgress, androidInstallUpdate, androidNotifyUpdate, isAndroid } from '~/utils/platform'

/**
 * Whether a newer Rivulet exists, and what this particular install can do with
 * that — the two are not the same question and are answered separately.
 *
 * `available` is a fact about GitHub: `app/utils/updates.ts` asks the API, and
 * every build can, including Android and a browser dev session. `capable` is a
 * fact about how the app got onto the machine, and comes from Rust — a copy
 * apt, pacman, dnf or Nix installed is not ours to overwrite (see
 * `can_self_update` in `src-tauri/src/lib.rs`). When the two disagree the panel
 * still says a release is out, and points at it instead of installing it.
 */
export const useUpdatesStore = defineStore('updates', () => {
  /** The running version. Empty in a browser, where there is no Tauri to ask. */
  const current = ref('')
  const release = ref<Update | null>(null)
  const capable = ref(false)
  const status = ref<'idle' | 'checking' | 'downloading' | 'ready' | 'failed'>('idle')
  const error = ref('')
  /** 0–1 while downloading. The bundle is ~100 MB, so this is worth showing. */
  const progress = ref(0)

  /**
   * A version the user waved off. Kept so the badge is a notification rather
   * than a permanent decoration — the About panel still offers it.
   */
  const skipped = useLocalStorage(key('updateSkipped'), '')

  const available = computed(() =>
    release.value && isNewer(current.value, release.value.version) ? release.value : null)

  const dismissed = computed(() => !!available.value && available.value.version === skipped.value)

  function dismiss() {
    skipped.value = available.value?.version ?? ''
  }

  /**
   * Once per launch, from `app.vue`. Never throws and never blocks anything:
   * offline is the ordinary case, not a failure worth a message.
   */
  async function check() {
    if (status.value === 'downloading' || status.value === 'ready')
      return
    status.value = 'checking'
    // Both of these fail in a browser-only dev session — `getVersion` rejects
    // and `invoke` has no backend — which leaves `current` empty and every
    // comparison false, so nothing is offered where nothing could be installed.
    current.value = await useTauriAppGetVersion().catch(() => '')
    capable.value = await invoke<boolean>('can_self_update').catch(() => false)
    release.value = await latestUpdate()
    status.value = 'idle'

    // On Android: notify the user when a new version is found (once per version).
    if (isAndroid() && available.value && !dismissed.value) {
      androidNotifyUpdate(available.value.version)
    }
  }

  /**
   * Re-check periodically while the app is open (every 6 hours).
   * Only runs on Android where `can_self_update` is false but we still
   * want to notify the user.
   */
  let recheckTimer: ReturnType<typeof setInterval> | null = null
  function startRecheckTimer() {
    if (recheckTimer)
      return
    const SIX_HOURS = 6 * 60 * 60 * 1000
    recheckTimer = setInterval(() => {
      if (!isAndroid())
        return
      check()
    }, SIX_HOURS)
  }

  /**
   * Download the new bundle and hand it to the platform's installer.
   *
   * On desktop this is the updater plugin's own `check()`, not the release we
   * already found: it reads the signed `latest.json` and carries the signature
   * the install is verified against. On Android it is a direct APK download
   * via DownloadManager.
   */
  async function install() {
    if (!available.value)
      return

    // ── Android: direct APK download + system install ────────────────
    if (isAndroid()) {
      const apkUrl = available.value.apk || available.value.url
      if (!apkUrl) {
        status.value = 'failed'
        error.value = $t('No downloadable APK found for this release.')
        return
      }
      status.value = 'downloading'
      progress.value = 0
      error.value = ''
      try {
        androidDismissUpdateNotification()
        const downloadId = androidDownloadUpdate(apkUrl)
        if (downloadId <= 0)
          throw new Error($t('Failed to start the download.'))

        // Poll until done or failed.
        await new Promise<void>((resolve, reject) => {
          const poll = setInterval(() => {
            const snap = androidGetUpdateProgress()
            if (!snap)
              return
            if (snap.total > 0)
              progress.value = snap.bytes / snap.total
            if (snap.done) {
              clearInterval(poll)
              progress.value = 1
              status.value = 'ready'
              resolve()
            }
          }, 500)
          // Safety timeout: 10 minutes — an APK is ~100 MB and a slow
          // connection may need a few minutes; the 5-minute default was
          // too tight when the OS needs a moment to transition from
          // STATUS_RUNNING to STATUS_SUCCESSFUL after the last byte.
          setTimeout(() => {
            clearInterval(poll)
            reject(new Error($t('Download timed out.')))
          }, 10 * 60 * 1000)
        })
      }
      catch (e) {
        status.value = 'failed'
        error.value = e instanceof Error ? e.message : String(e)
      }
      return
    }

    // ── Desktop: updater plugin ──────────────────────────────────────
    if (!capable.value)
      return
    status.value = 'downloading'
    progress.value = 0
    error.value = ''
    try {
      const update = await useTauriUpdaterCheck()
      if (!update)
        throw new Error($t('The release carries no update for this platform.'))

      let total = 0
      let done = 0
      await update.downloadAndInstall(event => {
        if (event.event === 'Started')
          total = event.data.contentLength ?? 0
        else if (event.event === 'Progress' && total)
          progress.value = (done += event.data.chunkLength) / total
        else if (event.event === 'Finished')
          progress.value = 1
      })
      // Windows never gets here: its installers require the app to be closed,
      // so the plugin exits the process partway through `downloadAndInstall`.
      status.value = 'ready'
    }
    catch (e) {
      status.value = 'failed'
      error.value = e instanceof Error ? e.message : String(e)
    }
  }

  /** On Android, open the downloaded APK so the system installer takes over. */
  function openInstaller() {
    androidInstallUpdate()
  }

  const restart = () => useTauriProcessRelaunch()

  return {
    current,
    release,
    capable,
    status,
    error,
    progress,
    available,
    dismissed,
    dismiss,
    check,
    install,
    openInstaller,
    restart,
    startRecheckTimer,
  }
})
