/**
 * Map the IptvError string from the Rust backend to a user-facing
 * translation key + optional interpolation values.
 *
 * The mapper returns a stable key — the same English sentence that
 * vue-i18n expects as a catalog entry. The literal `$t('...')` calls
 * at the bottom of this file are not invoked at runtime; they exist
 * so `bun scripts/i18n.ts` finds the literals and writes the catalog
 * entries on the next regen. Call sites render the error as
 * `{{ $t(errorKey, errorValues) }}`; the resolver walks the catalog
 * by literal first, then by dotted path (see i18n/i18n.config.ts).
 */

export interface IptvErrorInfo {
  key: string
  values?: Record<string, string | number>
}

export function mapIptvError(raw: string, context: 'xtream' | 'playlist' = 'xtream'): IptvErrorInfo {
  const msg = raw.toLowerCase()

  // Match the IptvError::Display prefixes the backend produces. Each
  // key here is also a `$t('...')` literal at the bottom of this
  // file so the i18n scanner picks it up.
  if (msg.includes('subscription has expired'))
    return { key: 'Your subscription has expired. Please renew with your provider.' }
  if (msg.includes('account has been disabled'))
    return { key: 'This account has been disabled. Please contact your provider.' }
  if (msg.includes('invalid credentials'))
    return { key: 'Invalid username or password. Please check your credentials.' }

  // Network errors: inspect the cause text.
  if (msg.includes('timed out') || msg.includes('timeout'))
    return { key: 'Connection timed out. Check your firewall or try a VPN.' }
  if (msg.includes('tls') || msg.includes('certificate') || msg.includes('ssl'))
    return { key: 'SSL/TLS error. The server may be blocking our requests.' }
  if (msg.includes('dns') || msg.includes('name resolve') || msg.includes('getaddrinfo'))
    return { key: 'Could not find the server. Check the URL.' }
  if (msg.includes('connection refused'))
    return { key: 'Connection refused. The server is not accepting requests on this port.' }
  if (msg.includes('http ')) {
    // Extract HTTP status code if present.
    const m = msg.match(/http\s+(\d+)/)
    if (m) {
      const code = m[1]
      if (code === '401' || code === '403')
        return { key: 'Invalid username or password. Please check your credentials.' }
      if (code === '404') {
        return context === 'playlist'
          ? { key: 'Playlist URL returned 404. Ask your provider for a current M3U link.' }
          : { key: 'Server returned 404. This may not be an Xtream endpoint.' }
      }
      if (code === '502' || code === '503' || code === '504')
        return { key: 'Server is temporarily unavailable. Try again in a moment.' }
      return { key: 'Server returned an error', values: { code: Number(code) } }
    }
  }

  // Fallback: show the raw message. The Vue template passes it
  // through `$t()` so the i18n scanner sees it on the next regen.
  return { key: raw }
}

// Anchors for the i18n scanner. The mapper above returns the
// English text of one of these as a key. None of these run at
// runtime; they exist so `bun scripts/i18n.ts` finds the literals
// and writes them to the 72 catalogs.
declare const $t: (key: string, values?: Record<string, string | number>) => string
if (false as boolean) {
  $t('Your subscription has expired. Please renew with your provider.')
  $t('This account has been disabled. Please contact your provider.')
  $t('Invalid username or password. Please check your credentials.')
  $t('Connection timed out. Check your firewall or try a VPN.')
  $t('SSL/TLS error. The server may be blocking our requests.')
  $t('Could not find the server. Check the URL.')
  $t('Connection refused. The server is not accepting requests on this port.')
  $t('Server returned 404. This may not be an Xtream endpoint.')
  $t('Playlist URL returned 404. Ask your provider for a current M3U link.')
  $t('Server is temporarily unavailable. Try again in a moment.')
  $t('Server returned an error')
}
