import { createGlobalState, useLocalStorage } from '@vueuse/core'
import { key } from '~/brand'

/**
 * How the Live TV grids lay a channel out: `list` is a guide row (number,
 * logo, name, what is on now), `grid` the logo tiles. Free and Premium share
 * the one choice, remembered per install — someone who reads a guide reads it
 * in both. Global so the toggle in a header and the grid under it are the
 * same ref, not two copies waiting on a storage event.
 */
export const useLiveLayout = createGlobalState(() =>
  useLocalStorage<'list' | 'grid'>(key('liveLayout'), 'list'),
)
