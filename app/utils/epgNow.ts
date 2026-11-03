interface Timed { start: string, stop?: string | null }

/**
 * What is on at `now`, else what is on next. A guide keeps a few hours behind
 * it ("what just ended"), so the first programme in the list is not the
 * current one; one with no end is taken to run an hour.
 */
export function currentProgram<T extends Timed>(list: T[], now: number): T | null {
  return list.find(p => {
    const start = new Date(p.start).getTime()
    const end = p.stop ? new Date(p.stop).getTime() : start + 3_600_000
    return end > now
  }) ?? null
}
