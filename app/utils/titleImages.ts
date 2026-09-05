import type { MediaType } from './tmdb'
import { tmdb } from './tmdb'

/** Stills for the title hero and Images row. Not part of the first detail request. */
export function useTitleImages(type: MaybeRefOrGetter<MediaType>, id: MaybeRefOrGetter<string>) {
  return useAsyncData(
    () => `images-${toValue(type)}-${toValue(id)}`,
    async () => {
      const tid = String(toValue(id) ?? '')
      if (!tid)
        return []
      const page = await tmdb<{ backdrops?: { file_path: string, vote_average?: number }[] }>(
        `/${toValue(type)}/${tid}/images`,
        { include_image_language: 'en,null' },
      )
      return (page.backdrops ?? [])
        .toSorted((a, b) => (b.vote_average ?? 0) - (a.vote_average ?? 0))
        .slice(0, 12)
        .map(b => b.file_path)
    },
    {
      lazy: true,
      server: false,
      immediate: false,
      watch: [() => toValue(type), () => toValue(id)],
    },
  )
}
