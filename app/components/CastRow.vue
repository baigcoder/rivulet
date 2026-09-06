<script setup lang="ts">
import type { Person } from '~/utils/tmdb'

defineProps<{ title: string, people: Person[] }>()

function armPerson(event: PointerEvent, id: number) {
  if (event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey)
    prefetchPerson(id)
}
</script>

<template>
  <scroll-row :title="title">
    <nuxt-link
      v-for="person in people"
      :key="person.id"
      :to="personLink(person.id)"
      no-prefetch
      class="group w-32 sm:w-36 shrink-0 outline-none"
      @pointerdown="armPerson($event, person.id)"
    >
      <!-- Same 2:3 frame as a poster card — profileUrl's w185 is 185x278. -->
      <div class="aspect-2/3 overflow-hidden rounded-2xl bg-surface-container shadow-md ring-1 ring-white/10 transition-transform duration-300 group-hover:scale-[1.03] group-hover:shadow-xl [&_img]:object-top">
        <media-poster :src="profileUrl(person.profile)" :alt="person.name" />
      </div>
      <div class="truncate pt-2.5 text-body-medium font-semibold text-on-surface group-hover:text-primary transition-colors" :title="person.name">
        {{ person.name }}
      </div>
      <div class="line-clamp-2 text-body-small text-on-surface-variant opacity-85 mt-0.5 leading-snug" :title="person.role">
        {{ person.role }}
      </div>
    </nuxt-link>
  </scroll-row>
</template>
