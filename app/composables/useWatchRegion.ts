/**
 * The country TMDB's watch-provider lists are read for. The app language
 * usually knows its region ("pt-BR"), and that is the right default; the US
 * catalogue — TMDB's largest — is the fallback. A region picked on a provider
 * page wins over both. Shared by Home's Streaming row and a title's "Where it
 * streams", so the two never disagree about where the user is.
 */
export function useWatchRegion() {
  const settings = useSettingsStore()
  return computed(() => {
    if (settings.watchRegion)
      return settings.watchRegion
    try {
      return new Intl.Locale(uiLocale()).maximize().region ?? 'US'
    }
    catch {
      return 'US'
    }
  })
}
