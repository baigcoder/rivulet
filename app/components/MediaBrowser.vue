<script setup lang="ts">
import type { MediaType } from '~/utils/tmdb'

/**
 * `anime` narrows every list to Japanese animation. TMDB has no anime endpoint —
 * the genre + original language pair is what it gives you, and it covers far
 * more than the "anime" keyword (210024), which most titles aren't tagged with.
 */
const props = defineProps<{
  type: MediaType
  anime?: boolean
  /** Starting category when the page was opened pre-filtered (?cat=…). */
  initialCategory?: string
  /**
   * Extra `/discover` params merged into every request — how the Streaming
   * pages narrow a feed to one watch provider without this file knowing what
   * a provider is.
   */
  extraParams?: Record<string, unknown>
  /** Replaces the standard category row when a page has its own vocabulary. */
  categoriesOverride?: { value: string, title: string }[]
  /** Title shown at the top of the page, e.g. "Popular TV". */
  pageTitle?: string
}>()

const route = useRoute()
const { lgAndUp } = useDisplay()

const isMovie = props.type === 'movie'

// Directly from the route — bypasses keepalive stale props
const pageTitle = computed(() => props.pageTitle ?? (String(route.query.label ?? '') || undefined))

const categories = computed(() => props.categoriesOverride ?? [
  { value: 'popular', title: $t('Popular') },
  // /trending takes no filters at all, so it can't be narrowed to anime.
  ...props.anime ? [] : [{ value: 'trending', title: $t('Trending') }],
  { value: 'top_rated', title: $t('Top rated') },
  isMovie
    ? { value: 'upcoming', title: $t('Upcoming') }
    : { value: 'on_the_air', title: $t('On the air') },
  isMovie
    ? { value: 'now_playing', title: $t('In cinemas') }
    : { value: 'airing_today', title: $t('Airing today') },
])

const category = ref(props.initialCategory ?? 'popular')
// `?genre=` is how Home's genre tiles and "See all" links arrive — read, and
// followed when keepalive brings the page back for a different one.
const genreFromRoute = () => Number(route.query.genre) || null
// `?keyword=` arrives from a title page's keyword chips: TMDB's own tags, so
// "heist" or "time travel" is one press from everything else tagged with it.
const keyword = computed(() => Number(route.query.keyword) || null)
const genre = ref<number | null>(genreFromRoute())
watch(() => route.query.genre, () => (genre.value = genreFromRoute()))

// keepalive reuses the component — sync when the route changes
watch(() => props.initialCategory, v => {
  category.value = v ?? 'popular'
})

const { data: genres } = useGenres(props.type)

const isTrending = computed(() => category.value === 'trending')

/** "Top rated · Comedy" — what the grid under the banner is, in words. */
const categoryTitle = computed(() => categories.value.find(c => c.value === category.value)?.title ?? '')
const genreName = computed(() => genres.value?.find(g => g.id === genre.value)?.name ?? '')
const listHeading = computed(() => [categoryTitle.value, genreName.value].filter(Boolean).join(' · '))
/** For the button that hides the filter on a phone — a set filter has to show. */
const filtered = computed(() => genre.value !== null || keyword.value !== null)

function day(offset = 0) {
  return new Date(Date.now() + offset * 864e5).toISOString().slice(0, 10)
}

// /discover carries the filters as query params; trending is its own endpoint
// and takes none, so a chosen genre rides along as `keepGenre` and the feed
// filters the fetched pages itself. Week rather than day — the day window is too
// volatile to be worth a control.
const request = computed(() => {
  if (isTrending.value && keyword.value === null)
    return { path: `/trending/${props.type}/week`, type: props.type, keepGenre: genre.value ?? undefined }

  // Sorting only accepts the release-date field; filtering a show by when it
  // airs needs air_date, which is a different field than first_air_date.
  const sortDate = isMovie ? 'primary_release_date' : 'first_air_date'
  const onDate = isMovie ? 'primary_release_date' : 'air_date'
  const sorting: Record<string, Record<string, unknown>> = {
    popular: { sort_by: 'popularity.desc' },
    top_rated: { 'sort_by': 'vote_average.desc', 'vote_count.gte': isMovie ? 300 : 150 },
    upcoming: { sort_by: `${sortDate}.asc`, [`${onDate}.gte`]: day(1) },
    now_playing: { sort_by: 'popularity.desc', [`${onDate}.gte`]: day(-45), [`${onDate}.lte`]: day() },
    on_the_air: { sort_by: 'popularity.desc', [`${onDate}.gte`]: day(), [`${onDate}.lte`]: day(7) },
    airing_today: { sort_by: 'popularity.desc', [`${onDate}.gte`]: day(), [`${onDate}.lte`]: day() },
    // A provider's "new": recently released, newest first — popularity is what
    // the first tab is already for.
    new: { sort_by: `${sortDate}.desc`, [`${onDate}.lte`]: day(), [`${onDate}.gte`]: day(-365) },
  }

  // Comma-joined ids are an AND, so a sub-genre stacks on top of Animation.
  const genres = [...new Set([props.anime ? ANIMATION : null, genre.value].filter(g => g !== null))]

  // Parental controls: filter by certification on discover endpoints (movies only;
  // TV content ratings are per-episode and not filterable via discover).
  const settings = useSettingsStore()
  const parentalParams: Record<string, unknown> = {}
  if (settings.parentalEnabled && isMovie) {
    parentalParams.certification_country = 'US'
    parentalParams['certification.lte'] = settings.parentalMaxRating
  }

  return {
    path: `/discover/${props.type}`,
    type: props.type,
    params: {
      include_adult: false,
      with_genres: genres.join(',') || undefined,
      with_keywords: keyword.value ?? undefined,
      with_original_language: props.anime ? 'ja' : undefined,
      ...sorting[category.value],
      ...parentalParams,
      ...props.extraParams,
    },
  }
})

const { items, pending, error, done, loadMore } = useMediaFeed(request)
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <options-bar :active="filtered ? 1 : 0">
      <div class="flex min-w-0 items-center gap-4">
        <h1 v-if="pageTitle" class="shrink-0 text-headline-small font-bold tracking-tight">
          {{ pageTitle }}
        </h1>
        <!-- Chips only from lg up, where the row has room for five of them beside
             the genre filter, poster slider and layout toggle. Below that — a
             phone, or a desktop window in the md–lg band — they'd crowd those
             controls off the bar, so the category collapses to a dropdown. It
             stays on the bar either way; the genre filter beside it is the one
             that goes behind the button once OptionsBar runs out of room. -->
        <v-chip-group
          v-if="lgAndUp"
          v-model="category"
          mandatory
          show-arrows
          selected-class="bg-primary text-on-primary font-medium"
        >
          <!-- Outlined: the tonal default left every unselected category close
               to invisible beside the selected one. -->
          <v-chip
            v-for="option in categories"
            :key="option.value"
            :value="option.value"
            :text="option.title"
            size="small"
            variant="outlined"
          />
        </v-chip-group>
        <v-select
          v-else
          v-model="category"
          :items="categories"
          item-title="title"
          item-value="value"
          class="max-w-52 shrink-0 md:w-52 md:max-w-none"
        />
      </div>

      <!-- Inline while the row can hold it, in a bottom sheet when it can't.
           The `md:` widths are the bar's; in the sheet it takes the full row. -->
      <template v-if="!lgAndUp" #filters>
        <v-select
          v-model="genre"
          :items="genres"
          item-title="name"
          item-value="id"
          :label="$t('Genre')"
          clearable
          class="w-52 shrink-0"
        />
      </template>
    </options-bar>

    <div class="min-h-0 flex-1">
      <media-layout
        :items="items"
        :pending="pending"
        :done="done"
        :error="error"
        @load="loadMore"
      >
        <template #header>
          <browse-spotlight :items="items" :label="listHeading" :genres="genres ?? []" />
          <genre-chips v-if="lgAndUp && genres?.length" v-model="genre" :genres="genres" class="mb-4" />
          <h2 v-if="items.length" class="mb-3 text-title-large font-bold">
            {{ listHeading }}
          </h2>
        </template>
      </media-layout>
    </div>
  </div>
</template>
