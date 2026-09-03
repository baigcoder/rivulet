import type { SubtitleStyle } from '~/utils/subtitles'
import {
  mdiAccountCircleOutline,
  mdiCastConnected,
  mdiFolderOutline,
  mdiInformationOutline,
  mdiPaletteOutline,
  mdiPowerPlugOutline,
  mdiShieldLockOutline,
  mdiSubtitlesOutline,
  mdiTranslate,
  mdiWifi,
} from '@mdi/js'
import { key } from '~/brand'
import { DEFAULT_SOURCE } from '~/theme/presets'

export type SectionKey = 'appearance' | 'language' | 'sources' | 'subtitles' | 'network' | 'storage' | 'account' | 'about' | 'parental' | 'premium-tv'

/**
 * The sidebar of the settings layout, in the order it lists them. A `value` is
 * also the route the section lives at (`/settings/<value>`), so this table is
 * the whole registry: the sidebar, the heading and the URLs all come off it.
 *
 * `title` is a function because this list is built when the module loads, which
 * is before there is a Nuxt context for `$t` to read a locale from — and
 * because the labels have to change when the language does, which a string
 * baked in at import time never would.
 */
export const SECTIONS: { value: SectionKey, title: () => string, icon: string }[] = [
  { value: 'appearance', title: () => $t('Appearance'), icon: mdiPaletteOutline },
  { value: 'language', title: () => $t('Language'), icon: mdiTranslate },
  { value: 'sources', title: () => $t('Sources'), icon: mdiPowerPlugOutline },
  { value: 'premium-tv', title: () => $t('Premium TV'), icon: mdiCastConnected },
  { value: 'subtitles', title: () => $t('Subtitles'), icon: mdiSubtitlesOutline },
  { value: 'network', title: () => $t('Network'), icon: mdiWifi },
  { value: 'storage', title: () => $t('Storage'), icon: mdiFolderOutline },
  { value: 'parental', title: () => $t('Parental controls'), icon: mdiShieldLockOutline },
  { value: 'account', title: () => $t('Account'), icon: mdiAccountCircleOutline },
  { value: 'about', title: () => $t('About'), icon: mdiInformationOutline },
]

/**
 * How much of the interface is allowed to move.
 *
 * Deliberately not folded into `reduceEffects`. That one governs *effects* — the
 * frosted blur behind the chrome, the blur on the backdrop art — and a television
 * wants those off. Motion is a different question with a different answer: the
 * reveals are keyframes on `transform` and `opacity`, which the compositor
 * handles without repainting, so a TV can afford them even where it cannot afford
 * a second blurred surface. One switch for both would have meant choosing which
 * of the two to get wrong there.
 *
 * `title` is a function for the same reason SECTIONS' is — the table is built
 * when the module loads, before `$t` has a locale to read.
 */
export type MotionMode = 'system' | 'full' | 'reduced' | 'none'

/** What the class on `<html>` can be; `system` has resolved by then. */
export type MotionTier = Exclude<MotionMode, 'system'>

export const MOTION_MODES: { value: MotionMode, title: () => string }[] = [
  { value: 'system', title: () => $t('Automatic') },
  { value: 'full', title: () => $t('Full') },
  { value: 'reduced', title: () => $t('Reduced') },
  { value: 'none', title: () => $t('Off') },
]

/**
 * Everything the settings page edits, kept in localStorage — there is no
 * account yet, so "local" is the only place settings can live.
 *
 * Kept in localStorage, like every other preference in the app, rather than
 * tauri-plugin-store. The webview's storage is per-app and survives updates;
 * the day settings have to sync to a backend, this store is the one thing that
 * changes.
 */
export const useSettingsStore = defineStore('settings', () => {
  /**
   * The UI language, as the bare code the locale list is keyed by: `sl`, not
   * `sl-SI`. Empty means "whatever the app opened in", which is the default
   * locale.
   *
   * This is only where the preference is *remembered* — @nuxtjs/i18n owns the live
   * one. app.vue is what marries the two: it restores this at boot and writes
   * it back whenever the locale changes. The module's own memory is a cookie,
   * which a `tauri://` origin does not reliably keep — and a `brand.` key
   * travels in a backup, which a cookie also would not.
   */
  const locale = useLocalStorage(key('locale'), '')

  // --- Appearance ---
  const theme = useLocalStorage(key('theme'), 'dark')
  /** The colour the "Your colour" themes are generated from (see theme/palette). */
  const source = useLocalStorage(key('themeSource'), DEFAULT_SOURCE)
  /**
   * Build the palette from whatever is on screen instead, re-reading it every
   * time the art changes. Drives the same two generated themes as `source`
   * does, so it costs no extra palette — see `app.vue`.
   */
  const themeFromArt = useLocalStorage(key('themeFromArt'), false)
  /**
   * Whether a picture of the user's own counts as "what's on screen". Off, only
   * a title's artwork moves the palette and the picture the app rests on leaves
   * the theme's own colours alone — which is the point of choosing a theme and a
   * background that go together.
   */
  const colourFromPicture = useLocalStorage(key('colourFromPicture'), false)
  /** Injected as a plain <style> tag, so it outranks everything in a layer. */
  const customCss = useLocalStorage(key('customCss'), '')
  /** Zoom for the whole interface. 1 = the sizes the app ships with. */
  const uiScale = useLocalStorage(key('uiScale'), 1)
  /**
   * Drop the effects that cost the most frames — see `.reduce-effects` in
   * assets/css/layers.css for exactly which. Defaults on for a television,
   * which is the hardware that needs it: the set this was measured on took ten
   * d-pad moves at 13fps with these effects and 23 without them.
   * `isTv()` reads a bridge Android installs before the page loads, so it
   * answers correctly the first time the store is built.
   */
  const reduceEffects = useLocalStorage(key('reduceEffects'), isTv() ?? false)

  /**
   * How much moves, and what decides it when the user hasn't said.
   *
   * `system` is the default and resolves to `reduced` on a television, or wherever
   * the operating system has been asked for less motion — which is the first time
   * this app has honoured `prefers-reduced-motion` at all. Any other value is a
   * standing override, because an explicit answer outranks a guess about the
   * device.
   *
   * The OS half is read reactively rather than once, so turning that setting on
   * takes effect without relaunching. `isTv()` can be read once: it comes from a
   * bridge Android installs before the page loads, and a television does not stop
   * being one.
   */
  const motion = useLocalStorage<MotionMode>(key('motion'), 'system')

  const prefersLessMotion = useMediaQuery('(prefers-reduced-motion: reduce)')

  /** The tier actually painted — one class on `<html>`, applied in app.vue. */
  const effectiveMotion = computed<MotionTier>(() =>
    motion.value !== 'system'
      ? motion.value
      : prefersLessMotion.value || isTv() === true ? 'reduced' : 'full')

  // --- Sources ---
  /**
   * Servers to search for something to play. Ships empty and stays empty until
   * the user adds one: the app comes with no sources and suggests none.
   */
  const sources = useLocalStorage<string[]>(key('sources'), [])

  /**
   * How the Play button starts a title.
   *
   * On: Play picks a magnet and the torrent engine streams it while the rest
   * of the file keeps downloading. Off: Play opens a Direct / debrid URL and
   * never touches the engine. Download and picking a release are not this
   * switch — they still file or play the row you named.
   */
  const allowTorrents = useLocalStorage(key('allowTorrents'), true)

  /**
   * The TMDB watch region the Streaming pages browse — provider catalogs are
   * per-country. '' means "take it from the app language" at the point of use.
   */
  const watchRegion = useLocalStorage(key('watchRegion'), '')

  // --- Film data ---
  /**
   * A TMDB read token of the user's own, used instead of the one the build
   * ships with. The bundled token sits in the client bundle where anyone can
   * read it, so it is one complaint away from being revoked — and a revoked
   * token is every installed copy losing artwork, titles and search at once.
   * This is the way back from that without waiting for a release.
   */
  const tmdbKey = useLocalStorage(key('tmdbKey'), '')

  // --- Network ---
  // MB/s, 0 meaning "work it out" (see `uploadLimit` in utils/torrents).
  const downLimit = useLocalStorage(key('downLimit'), 0)
  const upLimit = useLocalStorage(key('upLimit'), 0)

  /** Android only — no other platform can tell a metered network from a free one. */
  const wifiOnly = useLocalStorage(key('wifiOnly'), false)

  // --- Storage ---
  /** Where torrents are written. '' = the app's own cache folder. */
  const downloadDir = useLocalStorage(key('downloadDir'), '')

  // --- Subtitles ---
  // mergeDefaults: a build that adds a property must not read `undefined` out
  // of the copy stored by the build before it.
  const subs = useLocalStorage<SubtitleStyle>(key('subStyle'), { ...SUBTITLE_DEFAULTS }, { mergeDefaults: true })

  function resetSubs() {
    subs.value = { ...SUBTITLE_DEFAULTS }
  }

  // --- Notifications ---
  const notifyComplete = useLocalStorage(key('notifyComplete'), true)
  const notifyError = useLocalStorage(key('notifyError'), true)

  // --- Parental controls ---
  const parentalEnabled = useLocalStorage(key('parentalEnabled'), false)
  const parentalMaxRating = useLocalStorage(key('parentalMaxRating'), 'R')
  const parentalPin = useLocalStorage(key('parentalPin'), '')

  // --- Premium TV ---
  /** Hide adult (18+) channels from the Premium TV channel list. */
  const hideAdultChannels = useLocalStorage(key('hideAdultChannels'), false)

  // --- Subscription ---
  /**
   * Local-only feature flag. The app has no user auth, no account model, no
   * server: who has Premium is a single localStorage value the user (or an
   * installer) sets. The Rust Premium API server reads the same two fields
   * on startup to decide whether the :3032 API comes up at all.
   */
  const subscriptionTier = useLocalStorage(key('subscriptionTier'), 'free')
  /** Unix epoch milliseconds. 0 = no expiry (treated as expired). */
  const subscriptionExpiresAt = useLocalStorage(key('subscriptionExpiresAt'), 0)
  const isPremium = computed(() =>
    subscriptionTier.value === 'premium'
    && subscriptionExpiresAt.value > Date.now(),
  )

  return { locale, theme, source, themeFromArt, colourFromPicture, customCss, uiScale, reduceEffects, motion, effectiveMotion, sources, allowTorrents, watchRegion, tmdbKey, downLimit, upLimit, wifiOnly, downloadDir, subs, resetSubs, notifyComplete, notifyError, parentalEnabled, parentalMaxRating, parentalPin, hideAdultChannels, subscriptionTier, subscriptionExpiresAt, isPremium }
})
