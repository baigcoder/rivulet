import type { Update } from '~/utils/updates'
import { invoke } from '@tauri-apps/api/core'
import { sendNotification } from '@tauri-apps/plugin-notification'
import { key } from '~/brand'
import { androidDismissUpdateNotification, androidDownloadUpdate, androidGetUpdateProgress, androidInstallUpdate, androidNotifyUpdate, isAndroid, isDesktop } from '~/utils/platform'

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
  /** Last version an OS notification was posted for, so a 6-hour recheck does not nag. */
  const notified = useLocalStorage(key('updateNotified'), '')
  /** Version whose APK is on disk — so Install survives a restart, and an old file is not offered as a new one. */
  const pendingApk = useLocalStorage(key('updateApkVersion'), '')

  const available = computed(() =>
    release.value && isNewer(current.value, release.value.version) ? release.value : null)

  const dismissed = computed(() => !!available.value && available.value.version === skipped.value)

  function dismiss() {
    skipped.value = available.value?.version ?? ''
    androidDismissUpdateNotification()
  }

  /**
   * One OS notification per version. The toolbar badge is separate and stays
   * until they open About or tap Not now.
   */
  function ping() {
    const next = available.value
    const settings = useSettingsStore()
    if (!next || dismissed.value || !settings.notifyUpdates || notified.value === next.version)
      return
    const title = $t('Rivulet {version} is out', { version: next.version })
    const body = next.notes.split('\n').find(line => line.trim()) || $t('Open Settings → About to update.')
    if (isAndroid()) {
      androidNotifyUpdate(next.version)
    }
    else if (isDesktop()) {
      try {
        sendNotification({ title, body })
      }
      catch {
        // No permission, or the plugin is missing in this build.
      }
    }
    notified.value = next.version
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

    // An APK already on disk for *this* release: skip the download and show
    // Install, which is what replaces the running package.
    if (isAndroid() && available.value && pendingApk.value === available.value.version) {
      const snap = androidGetUpdateProgress()
      if (snap?.done && snap.path)
        status.value = 'ready'
    }

    ping()
  }

  /**
   * Re-check while the app is open so a release published after launch still
   * surfaces — desktop and Android both, the badge and the OS notification.
   */
  let recheckTimer: ReturnType<typeof setInterval> | null = null
  function startRecheckTimer() {
    if (recheckTimer)
      return
    const SIX_HOURS = 6 * 60 * 60 * 1000
    recheckTimer = setInterval(check, SIX_HOURS)
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
      pendingApk.value = available.value.version
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

  function installErrorFor(reason?: string) {
    switch (reason) {
      case 'signing_mismatch':
        return $t('This copy was built or sideloaded with a different signing key than the release APK. Uninstall Rivulet from Android Settings, then tap Install now again. Export a backup first from Settings → Account if you want to keep your library.')
      case 'not_newer':
        return $t('The downloaded APK is not newer than what is already installed.')
      case 'unreadable':
        return $t('The downloaded file does not look like a valid APK. Try downloading again.')
      case 'no_permission':
        return $t('Android needs permission to install updates. Turn on Install unknown apps for Rivulet, then try again.')
      default:
        return $t('No update file was found on disk. Download the APK again.')
    }
  }

  /** On Android, open the downloaded APK so the system installer takes over. */
  function openInstaller() {
    const result = androidInstallUpdate()
    if (!result.started) {
      status.value = 'failed'
      error.value = installErrorFor(result.reason)
    }
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
