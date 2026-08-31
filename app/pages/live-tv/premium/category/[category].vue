<script setup lang="ts">
/**
 * One category, deep-linkable.
 *
 * The route segment is the category **name**, which is what the counts
 * endpoint groups by and what `/channels?category=` filters on. Ids were
 * wrong for both: an M3U has no category ids at all, so half of a
 * provider's catalog was unreachable by URL.
 */
import { computed, onMounted, watch } from 'vue'

definePageMeta({ layout: 'default' })

const route = useRoute()
const premium = usePremiumTvStore()

const categoryName = computed(() => decodeURIComponent(String(route.params.category ?? '')))

async function apply(): Promise<void> {
  await premium.ensureLoaded()
  if (!premium.connected)
    return
  premium.setCategory(categoryName.value)
}

onMounted(apply)
watch(categoryName, apply)
</script>

<template>
  <!-- No heading of its own: the browser's header already names the
       category and carries the way back out of it. -->
  <div class="flex h-full min-h-0 flex-col px-4 py-4 md:px-6">
    <premium-tv-premium-browser show-back />
  </div>
</template>
