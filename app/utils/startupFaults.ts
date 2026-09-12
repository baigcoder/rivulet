import { invoke, isTauri } from '@tauri-apps/api/core'

/**
 * Local servers that could not start, in words a viewer can act on.
 *
 * Rivulet serves Free TV and Premium TV from fixed loopback ports. When one is
 * already taken — most often by a player process an earlier run left behind —
 * the server never starts, and every page that asks it something simply waits.
 * A spinner is the worst possible account of that, because it names nothing.
 *
 * Empty off Tauri, and empty when nothing failed, which is the usual case.
 */
export async function startupFaults(): Promise<string[]> {
  if (!isTauri())
    return []
  try {
    return await invoke<string[]>('startup_faults')
  }
  catch {
    // An older build has no such command. Nothing to report is the right
    // answer here — this is a diagnostic, and it must never be the thing
    // that breaks a page.
    return []
  }
}
