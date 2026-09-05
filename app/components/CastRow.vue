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
      class="w-28 shrink-0 outline-none"
      @pointerdown="armPerson($event, person.id)"
    >
      <!-- Same 2:3 frame as a poster card — profileUrl's w185 is 185x278. -->
      <div class="aspect-2/3 overflow-hidden rounded-xl bg-surface-container [&_img]:object-top">
        <media-poster :src="profileUrl(person.profile)" :alt="person.name" />
      </div>
      <div class="truncate pt-2 text-label-medium text-on-surface" :title="person.name">
        {{ person.name }}
      </div>
      <div class="line-clamp-2 text-label-small text-on-surface opacity-80" :title="person.role">
        {{ person.role }}
      </div>
    </nuxt-link>
  </scroll-row>
</template>
