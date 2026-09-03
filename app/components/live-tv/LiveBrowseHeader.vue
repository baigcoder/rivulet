<script setup lang="ts">
/**
 * Free and Premium share this row so they stay the same app.
 *
 * A surface plate like the library options bar, not a line of glyphs on
 * the page background. Search takes the leftover width (the old row left
 * a void between the title and a 18rem outlined box). Tools sit in one
 * tray so a remote walks them as a cluster. AppBar already has Back; the
 * one here is the in-page hop to the hub, sized like the rest of the tray.
 */
import { mdiArrowLeft, mdiRefresh, mdiTune } from '@mdi/js'

defineProps<{
  heading: string
  count: number
  /** Overrides the default "{count} channels" line. */
  countLine?: string
  /** Overrides the default search placeholder. */
  searchPlaceholder?: string
  /** Overrides the tune button label (Categories). */
  tuneLabel?: string
  statusTone: string
  statusLabel: string
  statusText: string
  statusMeta?: string
  showClear?: boolean
  refreshing?: boolean
  showTune?: boolean
}>()

const emit = defineEmits<{
  back: []
  clear: []
  refresh: []
  tune: []
}>()

const search = defineModel<string>('search', { required: true })
</script>

<template>
  <header class="flex flex-wrap items-center gap-2 rounded-2xl bg-surface-container/50 p-2">
    <div class="flex min-w-0 flex-1 items-center gap-1 sm:flex-none">
      <button
        type="button"
        class="grid size-11 shrink-0 place-items-center rounded-xl text-on-surface/70 transition-colors hover:bg-surface-container-highest hover:text-on-surface focus-visible:bg-surface-container-highest focus-visible:text-on-surface"
        :aria-label="$t('Back')"
        @click="emit('back')"
      >
        <v-icon :icon="mdiArrowLeft" size="22" />
      </button>

      <div class="min-w-0 pe-2">
        <h1 class="truncate text-title-large font-bold tracking-tight">
          {{ heading }}
        </h1>
        <p class="flex min-w-0 items-center gap-2 text-body-medium">
          <span class="truncate tabular-nums opacity-60">
            {{ countLine ?? $t('{count} channels', { count: count.toLocaleString() }) }}
          </span>
          <button
            v-if="showClear"
            type="button"
            class="inline-flex min-h-8 shrink-0 items-center rounded-full bg-primary/15 px-2.5 text-label-medium font-medium text-primary transition-colors hover:bg-primary/25 focus-visible:bg-primary/25"
            :aria-label="$t('Clear filters')"
            @click="emit('clear')"
          >
            {{ $t('Clear') }}
          </button>
        </p>
      </div>
    </div>

    <search-field
      v-model="search"
      :placeholder="searchPlaceholder ?? $t('Search channels')"
      density="default"
      class="order-last min-w-0 w-full sm:order-none sm:min-w-48 sm:flex-1"
    />

    <div class="flex shrink-0 items-center gap-0.5 rounded-xl bg-surface-container-high p-0.5">
      <div
        class="hidden min-h-11 max-w-52 items-center gap-2 px-2.5 md:flex"
        role="status"
        :aria-label="statusLabel"
        :title="statusLabel"
      >
        <span class="size-2 shrink-0 rounded-full" :class="statusTone" />
        <span class="min-w-0 truncate text-body-medium">{{ statusText }}</span>
        <span v-if="statusMeta" class="shrink-0 text-body-medium opacity-55">{{ statusMeta }}</span>
      </div>
      <span
        class="mx-2 size-2 shrink-0 rounded-full md:hidden"
        :class="statusTone"
        role="img"
        :aria-label="statusLabel"
        :title="statusLabel"
      />

      <slot />

      <button
        type="button"
        class="grid size-11 shrink-0 place-items-center rounded-lg text-on-surface/70 transition-colors hover:bg-surface-container-highest hover:text-on-surface focus-visible:bg-surface-container-highest focus-visible:text-on-surface disabled:opacity-40"
        :aria-label="$t('Refresh')"
        :disabled="refreshing"
        @click="emit('refresh')"
      >
        <v-icon :icon="mdiRefresh" size="22" :class="refreshing ? 'animate-spin' : undefined" />
      </button>
      <button
        v-if="showTune"
        type="button"
        class="grid size-11 shrink-0 place-items-center rounded-lg text-on-surface/70 transition-colors hover:bg-surface-container-highest hover:text-on-surface focus-visible:bg-surface-container-highest focus-visible:text-on-surface"
        :aria-label="tuneLabel ?? $t('Categories')"
        @click="emit('tune')"
      >
        <v-icon :icon="mdiTune" size="22" />
      </button>

      <slot name="end" />
    </div>

    <slot name="below" />
  </header>
</template>
