<script setup lang="ts">
import type { MediaDetail, MediaType, TitleExtras } from '~/utils/tmdb'
import { mdiTelevisionPlay } from '@mdi/js'

/**
 * The reference half of a title page: which services carry it, the facts a
 * film buff looks for, the studio logos, and TMDB's keywords as ways out.
 *
 * "Where it streams" is TMDB metadata, exactly like Home's Streaming row — it
 * links to that service's page in this app and says nothing about the user's
 * sources, which are the only thing that decides what plays.
 */
const props = defineProps<{
  type: MediaType
  media: MediaDetail
  extras: TitleExtras | null
  region: string
}>()

const languageName = computed(() => {
  const code = props.extras?.originalLanguage
  if (!code)
    return ''
  try {
    return new Intl.DisplayNames([uiLocale()], { type: 'language' }).of(code) ?? code
  }
  catch {
    return code
  }
})

const releaseText = computed(() => {
  const iso = props.extras?.released || props.media.released
  if (!iso)
    return ''
  try {
    return new Intl.DateTimeFormat(uiLocale(), { dateStyle: 'long' }).format(new Date(`${iso}T00:00:00`))
  }
  catch {
    return iso
  }
})

const facts = computed(() => {
  const m = props.media
  const x = props.extras
  return [
    { label: $t('Original title'), value: x?.originalTitle && x.originalTitle !== m.title ? x.originalTitle : '' },
    { label: $t('Original language'), value: languageName.value },
    { label: props.type === 'movie' ? $t('Released') : $t('First aired'), value: releaseText.value },
    { label: $t('Status'), value: m.status },
    { label: $t('Runtime'), value: runtimeText(m.runtime) },
    { label: $t('Budget'), value: moneyText(m.budget) },
    { label: $t('Box office'), value: moneyText(m.revenue) },
  ].filter(f => f.value)
})

function providerLink(p: { id: number, name: string, logo: string | null }) {
  return { path: `/streaming/${p.id}`, query: { name: p.name, logo: p.logo ?? '' } }
}

function keywordLink(k: { id: number, name: string }) {
  return { path: localePath(props.type === 'movie' ? '/movies' : '/tv'), query: { keyword: k.id, label: k.name } }
}
</script>

<template>
  <section class="mx-4 grid gap-8 md:mx-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
    <div class="flex flex-col gap-6">
      <div v-if="extras?.providers.length" class="flex flex-col gap-3">
        <h2 class="flex items-center gap-2 text-title-large font-bold">
          <v-icon :icon="mdiTelevisionPlay" size="22" class="opacity-70" />
          {{ $t('Where it streams') }}
          <span class="text-body-small font-normal opacity-50">{{ region }}</span>
        </h2>
        <div class="flex flex-wrap gap-3">
          <nuxt-link
            v-for="p in extras.providers"
            :key="p.id"
            :to="providerLink(p)"
            class="flex items-center gap-2.5 rounded-xl border border-white/10 bg-surface-container-high/70 py-1.5 pe-4 ps-1.5 outline-none transition-[border-color] hover:border-white/40 focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary"
          >
            <img v-if="p.logo" :src="logoUrl(p.logo, 'w300') ?? ''" alt="" class="size-9 rounded-lg object-cover">
            <span class="text-label-large font-semibold">{{ p.name }}</span>
          </nuxt-link>
        </div>
        <p class="text-body-small opacity-50">
          {{ $t('Listings from TMDB. What plays here comes from your own sources.') }}
        </p>
      </div>

      <div v-if="extras?.keywords.length" class="flex flex-col gap-3">
        <h2 class="text-title-large font-bold">
          {{ $t('Keywords') }}
        </h2>
        <div class="flex flex-wrap gap-2">
          <v-chip
            v-for="k in extras.keywords"
            :key="k.id"
            :to="keywordLink(k)"
            size="small"
            variant="outlined"
            :text="k.name"
          />
        </div>
      </div>
    </div>

    <div class="flex flex-col gap-6">
      <div v-if="facts.length" class="flex flex-col gap-3">
        <h2 class="text-title-large font-bold">
          {{ $t('Details') }}
        </h2>
        <dl class="grid grid-cols-1 gap-x-8 gap-y-3 rounded-2xl border border-white/10 bg-surface-container-high/40 p-5 sm:grid-cols-2">
          <div v-for="f in facts" :key="f.label" class="flex min-w-0 flex-col">
            <dt class="text-label-medium uppercase tracking-wider opacity-55">
              {{ f.label }}
            </dt>
            <dd class="ms-0 truncate text-body-large">
              {{ f.value }}
            </dd>
          </div>
        </dl>
      </div>

      <div v-if="extras?.companies.length" class="flex flex-wrap items-center gap-x-6 gap-y-4">
        <template v-for="c in extras.companies" :key="c.id">
          <!-- Logos are drawn for a white page; inverted they read on a dark one. -->
          <img
            v-if="c.logo"
            :src="logoUrl(c.logo, 'w300') ?? ''"
            :alt="c.name"
            :title="c.name"
            class="h-7 w-auto max-w-32 object-contain opacity-80 brightness-0 invert"
          >
          <span v-else class="text-label-large font-semibold opacity-70">{{ c.name }}</span>
        </template>
      </div>
    </div>
  </section>
</template>
