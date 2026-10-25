import type { MediaType } from './tmdb'
import { tmdbLanguage } from './i18n'
import { tmdb } from './tmdb'

export interface TitleImages {
  stills: string[]
  logo: string | null
}

/** Old cache was a string[]; the logo change stores `{ stills, logo }`. */
export function titleImagesOf(value: unknown): TitleImages {
  if (Array.isArray(value))
    return { stills: value.filter((p): p is string => typeof p === 'string'), logo: null }
  if (value && typeof value === 'object' && Array.isArray((value as TitleImages).stills)) {
    const v = value as TitleImages
    return { stills: v.stills, logo: v.logo ?? null }
  }
  return { stills: [], logo: null }
}

/**
 * The title treatment to draw: the reader's own language first, then English,
 * then one with no words to read, then whatever the film has — the best-rated
 * within each, since a title often carries a dozen near-identical uploads.
 */
export function bestLogo(logos: { file_path: string, iso_639_1: string | null, vote_average?: number }[], lang = tmdbLanguage().split('-')[0]!): string | null {
  const order = (l: { iso_639_1: string | null }) =>
    l.iso_639_1 === lang ? 0 : l.iso_639_1 === 'en' ? 1 : l.iso_639_1 == null ? 2 : 3
  return logos.toSorted((a, b) => order(a) - order(b) || (b.vote_average ?? 0) - (a.vote_average ?? 0))[0]?.file_path ?? null
}

const logoCache = new Map<string, Promise<string | null>>()

/**
 * Just the title treatment, for the Home spotlight: it changes title every six
 * seconds and comes back round to the same ten, so each is asked for once.
 * Logos only — `/images` without a language filter is every poster anyone
 * uploaded, which is the request check:perf keeps off the first paint.
 */
export function loadTitleLogo(type: MediaType, id: string | number, original?: string) {
  const key = `${type}-${id}`
  const hit = logoCache.get(key)
  if (hit)
    return hit
  const pending = tmdb<{ logos?: { file_path: string, iso_639_1: string | null, vote_average?: number }[] }>(
    `/${type}/${id}/images`,
    { include_image_language: [...new Set([tmdbLanguage().split('-')[0], 'en', original, 'null'].filter(Boolean))].join(',') },
  )
    .then(page => bestLogo(page.logos ?? []))
    .catch(() => {
      logoCache.delete(key)
      return null
    })
  logoCache.set(key, pending)
  return pending
}

/** Stills + title logo. Not part of the first detail request. */
export function useTitleImages(type: MaybeRefOrGetter<MediaType>, id: MaybeRefOrGetter<string>, original?: MaybeRefOrGetter<string | undefined>) {
  const fetched = useAsyncData(
    () => `images-${toValue(type)}-${toValue(id)}`,
    async (): Promise<TitleImages> => {
      const tid = String(toValue(id) ?? '')
      if (!tid)
        return { stills: [], logo: null }
      const page = await tmdb<{
        backdrops?: { file_path: string, vote_average?: number }[]
        logos?: { file_path: string, iso_639_1: string | null, vote_average?: number }[]
      }>(
        `/${toValue(type)}/${tid}/images`,
        // The reader's language, English, none, and the film's own: 'en,null'
        // alone left a film whose only title treatment is in its own script
        // (most Hindi, Korean and Japanese releases) with no logo, and the
        // pause screen fell back to plain text. Not every language — that is
        // every poster anyone uploaded, which this request exists to avoid.
        { include_image_language: [...new Set([tmdbLanguage().split('-')[0], 'en', toValue(original), 'null'].filter(Boolean))].join(',') },
      )
      const logos = page.logos ?? []
      return {
        stills: (page.backdrops ?? [])
          .toSorted((a, b) => (b.vote_average ?? 0) - (a.vote_average ?? 0))
          .slice(0, 12)
          .map(b => b.file_path),
        logo: bestLogo(logos),
      }
    },
    {
      lazy: true,
      server: false,
      immediate: false,
      watch: [() => toValue(type), () => toValue(id)],
    },
  )
  const data = computed(() => titleImagesOf(fetched.data.value))
  return { ...fetched, data }
}
