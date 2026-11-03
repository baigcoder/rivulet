import type { IPTVChannel, EpgProgram as PremiumProgram } from '~/types/premium'
import type { LiveChannel } from '~/utils/iptv'
import { categoryLabel } from '~/utils/categoryLabel'
import { parseChannelName } from '~/utils/channelName'
import { currentProgram } from '~/utils/epgNow'

/** Times in epoch milliseconds. */
export interface FeaturedProgramme {
  title: string
  description?: string | null
  start: number
  stop?: number | null
}

/**
 * What the featured banner draws for one channel. Free and Premium keep their
 * guides in different shapes (ISO strings against epoch seconds), so each has
 * its own adapter onto the banner's milliseconds, shared by every page that
 * shows one.
 */
export interface FeaturedChannel<C> {
  channel: C
  name: string
  quality: string
  subtitle: string
  tint: string
  now: FeaturedProgramme | null
  next: FeaturedProgramme | null
}

interface FreeProgram { title: string, description?: string | null, start: string, stop?: string | null }

function subtitleOf(ch: { country?: string | null, categoryName?: string | null }): string {
  return ch.country || (ch.categoryName ? categoryLabel(ch.categoryName) : '')
}

export function featuredFromFree(ch: LiveChannel, guide: FreeProgram[], now = Date.now()): FeaturedChannel<LiveChannel> {
  const ms = (p: FreeProgram): FeaturedProgramme => ({
    title: p.title,
    description: p.description,
    start: Date.parse(p.start),
    stop: p.stop ? Date.parse(p.stop) : null,
  })
  const on = currentProgram(guide, now)
  const after = on ? guide[guide.indexOf(on) + 1] : undefined
  const parsed = parseChannelName(ch.name, ch.name)
  return {
    channel: ch,
    name: parsed.name,
    quality: parsed.quality,
    subtitle: subtitleOf(ch),
    tint: ch.categoryName || ch.id,
    now: on ? ms(on) : null,
    next: after ? ms(after) : null,
  }
}

export function featuredFromPremium(
  ch: IPTVChannel,
  guide: { now: PremiumProgram | null, next: PremiumProgram | null },
): FeaturedChannel<IPTVChannel> {
  const ms = (p: PremiumProgram): FeaturedProgramme => ({
    title: p.title,
    description: p.description,
    start: p.start * 1000,
    stop: p.stop ? p.stop * 1000 : null,
  })
  const parsed = parseChannelName(ch.name || '', ch.name || ch.id)
  return {
    channel: ch,
    name: parsed.name,
    quality: ch.quality || parsed.quality,
    subtitle: subtitleOf(ch),
    tint: ch.categoryName || ch.id,
    now: guide.now ? ms(guide.now) : null,
    next: guide.next ? ms(guide.next) : null,
  }
}
