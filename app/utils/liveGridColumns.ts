/** Gap between channel tiles — matches `gap-3` in the grids. */
export const LIVE_GRID_GAP = 12

/**
 * How many 16:9 tiles fit in a row. Drives both the virtualizer's slice
 * and `gridTemplateColumns`, because Tailwind breakpoints disagree with
 * the scroll container's actual width once a sidebar is open.
 */
export function liveGridColumnCount(
  width: number,
  density: 'compact' | 'comfortable' = 'comfortable',
): number {
  if (width < 1)
    return density === 'compact' ? 3 : 2
  const target = density === 'compact' ? 176 : 228
  const min = density === 'compact' ? 2 : 2
  return Math.max(min, Math.min(density === 'compact' ? 8 : 7, Math.floor((width + LIVE_GRID_GAP) / (target + LIVE_GRID_GAP))))
}

/** Rough row height for the virtualizer before rows are measured. */
export function liveGridRowEstimate(
  width: number,
  cols: number,
  density: 'compact' | 'comfortable',
  withCaption: boolean,
): number {
  if (width < 1 || cols < 1)
    return density === 'compact' ? 200 : 248
  const cardW = (width - LIVE_GRID_GAP * (cols - 1)) / cols
  const artH = cardW * 9 / 16
  if (!withCaption)
    return artH + LIVE_GRID_GAP + 4
  const meta = density === 'compact' ? 48 : 58
  return artH + meta + LIVE_GRID_GAP
}
