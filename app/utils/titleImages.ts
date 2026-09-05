import type { MediaType } from './tmdb'
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

/** Stills + title logo. Not part of the first detail request. */
export function useTitleImages(type: MaybeRefOrGetter<MediaType>, id: MaybeRefOrGetter<string>) {
  const fetched = useAsyncData(
    () => `images-${toValue(type)}-${toValue(id)}`,
    async (): Promise<TitleImages> => {
      const tid = String(toValue(id) ?? '')
      if (!tid)
        return { stills: [], logo: null }
      const page = await tmdb<{
        backdrops?: { file_path: string, vote_average?: number }[]
        logos?: { file_path: string, iso_639_1: string | null }[]
      }>(
        `/${toValue(type)}/${tid}/images`,
        { include_image_language: 'en,null' },
      )
      const logos = page.logos ?? []
      return {
        stills: (page.backdrops ?? [])
          .toSorted((a, b) => (b.vote_average ?? 0) - (a.vote_average ?? 0))
          .slice(0, 12)
          .map(b => b.file_path),
        logo: logos.find(l => l.iso_639_1 === 'en')?.file_path
          ?? logos[0]?.file_path
          ?? null,
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
