import type { Media, MediaType, TmdbPage } from '~/utils/tmdb'

export interface FeedRequest {
  path: string
  params?: Record<string, unknown>
  /** Fallback media type for endpoints that don't return media_type. */
  type?: MediaType
  /**
   * Genre id kept client-side. Only /trending needs it — that endpoint ignores
   * `with_genres`, so the genre can't be a query param; each page is fetched
   * whole and non-matching items dropped here. The list's own infinite scroll
   * notices the short grid and keeps pulling pages until it fills or runs out.
   * Not a query param, so it never reaches TMDB.
   */
  keepGenre?: number
}

/**
 * Paged TMDB list that grows as you scroll. Changing anything in `request`
 * (genre, category, query…) drops the old results and starts over; a null
 * request means "nothing to ask for yet", e.g. an empty search box.
 */
export function useMediaFeed(request: MaybeRefOrGetter<FeedRequest | null>) {
  const items = ref<Media[]>([])
  const pending = ref(false)
  const error = ref<string>()
  const page = ref(0)
  const totalPages = ref(1)
  const done = computed(() => page.value >= totalPages.value)

  // Bumped on reset so a response from the previous filter can't append to the
  // new list, and the ids already rendered — TMDB repeats items across pages.
  let generation = 0
  const seen = new Set<number>()

  async function loadMore() {
    if (pending.value || done.value)
      return

    const request_ = toValue(request)
    if (!request_)
      return

    const mine = generation
    pending.value = true
    error.value = undefined

    try {
      const nextPage = page.value + 1
      const { results, total_pages } = await tmdb<TmdbPage>(request_.path, {
        ...request_.params,
        page: nextPage,
      })

      if (mine !== generation)
        return

      page.value = nextPage
      totalPages.value = total_pages

      const mapped = results
        .map(item => toMedia(item, request_.type))
        .filter((m): m is Media => m != null)
        .filter(m => !seen.has(m.id))
        .filter(m => request_.keepGenre ? m.genreIds.includes(request_.keepGenre) : true)

      for (const m of mapped)
        seen.add(m.id)

      items.value.push(...mapped)
    }
    catch (e) {
      if (mine === generation)
        error.value = e instanceof Error ? e.message : String(e)
    }
    finally {
      if (mine === generation)
        pending.value = false
    }
  }

  function reset() {
    generation++
    seen.clear()
    items.value = []
    page.value = 0
    totalPages.value = 1
    pending.value = false
    error.value = undefined
  }

  watch(() => JSON.stringify(toValue(request)), () => {
    reset()
    loadMore()
  }, { immediate: true })

  return { items, pending, error, done, loadMore, reset }
}
