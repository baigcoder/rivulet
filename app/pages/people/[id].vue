<script setup lang="ts">
import { mdiAlertCircleOutline, mdiArrowLeft, mdiOpenInNew } from '@mdi/js'

definePageMeta({
  validate: ({ params }) => /^\d+$/.test(String((params as Record<string, string>).id)),
})

const route = useRoute()
const ui = useUiStore()

const id = computed(() => String((route.params as Record<string, string>).id))

const { data: person, pending, error } = usePersonDetail(id)
const { data: credits } = usePersonCredits(id)

const castByDepartment = computed(() => {
  if (!credits.value)
    return []
  const departments = new Map<string, typeof credits.value.cast>()
  for (const c of credits.value.cast) {
    const dept = c.department || 'Acting'
    if (!departments.has(dept))
      departments.set(dept, [])
    departments.get(dept)!.push(c)
  }
  return [...departments.entries()].map(([dept, items]) => ({ dept, items }))
})

const crewByDepartment = computed(() => {
  if (!credits.value)
    return []
  const departments = new Map<string, typeof credits.value.crew>()
  for (const c of credits.value.crew) {
    if (!departments.has(c.department))
      departments.set(c.department, [])
    departments.get(c.department)!.push(c)
  }
  return [...departments.entries()].map(([dept, items]) => ({ dept, items }))
})

function formatAge(birthday: string | null, deathday: string | null) {
  if (!birthday)
    return ''
  const start = new Date(birthday)
  const end = deathday ? new Date(deathday) : new Date()
  const age = Math.floor((end.getTime() - start.getTime()) / (365.25 * 24 * 60 * 60 * 1000))
  return deathday ? `${age}` : `${age}`
}
</script>

<template>
  <div class="h-full overflow-y-auto pb-12">
    <div v-if="error" class="flex h-full flex-col items-center justify-center gap-2">
      <v-icon :icon="mdiAlertCircleOutline" color="error" size="40" />
      <span class="text-body-medium opacity-70">{{ $t('Couldn\'t load this person.') }}</span>
      <v-btn variant="tonal" :to="localePath('/')">
        {{ $t('Back to home') }}
      </v-btn>
    </div>

    <template v-else>
      <section class="px-4 pb-8 pt-4 md:px-6">
        <v-btn :prepend-icon="mdiArrowLeft" variant="text" size="small" class="mb-3 -ml-2" @click="$router.back()">
          {{ $t('Back') }}
        </v-btn>

        <div class="flex flex-col gap-6 sm:flex-row sm:items-start">
          <div class="aspect-2/3 w-36 shrink-0 overflow-hidden rounded-2xl shadow-2xl sm:w-44">
            <media-poster :src="profileUrl(person?.profile, 'w185')" :alt="person?.name" />
          </div>

          <div class="flex min-w-0 flex-1 flex-col gap-3">
            <h1 class="text-headline-large font-bold drop-shadow-[0_2px_24px_rgba(0,0,0,0.6)]">
              {{ person?.name ?? $t('Loading…') }}
            </h1>

            <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-body-small opacity-75">
              <span v-if="person?.knownForDepartment">{{ person.knownForDepartment }}</span>
              <span v-if="person?.birthday">
                {{ $t('Born') }} {{ dateText(person.birthday) }}
                <template v-if="person.deathday"> — {{ $t('Died') }} {{ dateText(person.deathday) }} ({{ $t('aged {age}', { age: formatAge(person.birthday, person.deathday) }) }})</template>
                <template v-else> ({{ $t('aged {age}', { age: formatAge(person.birthday, null) }) }})</template>
              </span>
              <span v-if="person?.placeOfBirth">{{ person.placeOfBirth }}</span>
            </div>

            <p v-if="person?.alsoKnownAs.length" class="text-body-small opacity-60">
              {{ $t('Also known as') }}: {{ person.alsoKnownAs.join(', ') }}
            </p>

            <div v-if="person?.homepage" class="flex items-center gap-1">
              <a :href="person.homepage" target="_blank" rel="noopener" class="text-primary text-body-small hover:underline">
                {{ person.homepage }}
                <v-icon :icon="mdiOpenInNew" size="12" class="inline" />
              </a>
            </div>
          </div>
        </div>
      </section>

      <section v-if="person?.biography" class="px-4 pb-6 md:px-6">
        <h2 class="text-title-medium mb-2 font-semibold">
          {{ $t('Biography') }}
        </h2>
        <p class="max-w-3xl whitespace-pre-line text-body-medium opacity-85">
          {{ person.biography }}
        </p>
      </section>

      <section v-if="castByDepartment.length" class="px-4 pb-6 md:px-6">
        <div v-for="{ dept, items } in castByDepartment" :key="dept" class="mb-6">
          <h2 class="text-title-medium mb-3 font-semibold">
            {{ dept }}
          </h2>
          <media-layout :items="items.map(c => c.media).filter(Boolean)" :pending="false" :done="true" />
        </div>
      </section>

      <section v-if="crewByDepartment.length" class="px-4 pb-6 md:px-6">
        <div v-for="{ dept, items } in crewByDepartment" :key="dept" class="mb-6">
          <h2 class="text-title-medium mb-3 font-semibold">
            {{ dept }}
          </h2>
          <media-layout :items="items.map(c => c.media).filter(Boolean)" :pending="false" :done="true" />
        </div>
      </section>
    </template>
  </div>
</template>
