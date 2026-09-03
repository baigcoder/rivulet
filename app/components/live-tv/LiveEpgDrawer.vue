<script setup lang="ts">
import type { EpgProgram, LiveChannel } from '~/utils/iptv'
import { mdiClose } from '@mdi/js'
import { proxyLogo } from '~/utils/premiumTv'

const props = defineProps<{
  show: boolean
  channel: LiveChannel | null
  programs: EpgProgram[]
}>()

const emit = defineEmits<{
  close: []
  play: [channel: LiveChannel, program?: EpgProgram]
}>()

const now = ref(Date.now())

const epgTimer = setInterval(() => {
  now.value = Date.now()
}, 30000)

onUnmounted(() => {
  clearInterval(epgTimer)
})

function programStatus(p: EpgProgram): 'past' | 'live' | 'future' {
  const start = new Date(p.start).getTime()
  const stop = p.stop ? new Date(p.stop).getTime() : start + 3600000
  if (now.value < start)
    return 'future'
  if (now.value > stop)
    return 'past'
  return 'live'
}

function programProgress(p: EpgProgram): number {
  const start = new Date(p.start).getTime()
  const stop = p.stop ? new Date(p.stop).getTime() : start + 3600000
  if (now.value < start)
    return 0
  if (now.value > stop)
    return 100
  return Math.round(((now.value - start) / (stop - start)) * 100)
}

function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
}

function formatTimeRange(p: EpgProgram): string {
  const s = formatTime(p.start)
  const e = p.stop ? formatTime(p.stop) : ''
  return e ? `${s} – ${e}` : s
}

const currentProgram = computed(() =>
  props.programs.find(p => programStatus(p) === 'live') ?? null,
)

const pastPrograms = computed(() =>
  props.programs.filter(p => programStatus(p) === 'past').slice(-10).reverse(),
)

const upcomingPrograms = computed(() =>
  props.programs.filter(p => programStatus(p) === 'future'),
)
</script>

<template>
  <teleport to="body">
    <transition name="drawer">
      <div
        v-if="show && channel"
        class="fixed inset-0 z-50 flex justify-end bg-black/40"
        @click.self="emit('close')"
      >
        <!-- Backdrop -->
        <div class="absolute inset-0 backdrop-blur-sm md:hidden" @click="emit('close')" />

        <!-- Panel -->
        <div class="relative z-10 flex h-full w-full flex-col border-l border-white/10 bg-surface-container md:w-[420px]">
          <!-- Header -->
          <div class="flex items-center gap-3 border-b border-white/10 px-5 py-4">
            <img
              v-if="channel.logoUrl"
              :src="proxyLogo(channel.logoUrl)"
              :alt="channel.name"
              class="size-10 rounded-lg object-contain"
            >
            <div class="min-w-0 flex-1">
              <h2 class="truncate text-title-large font-bold">
                {{ channel.name }}
              </h2>
              <p v-if="channel.categoryName" class="text-body-small opacity-50">
                {{ channel.categoryName }}
              </p>
            </div>
            <v-btn :icon="mdiClose" variant="text" size="small" @click="emit('close')" />
          </div>

          <!-- Now playing -->
          <div v-if="currentProgram" class="border-b border-white/10 px-5 py-4">
            <div class="mb-1 flex items-center gap-2">
              <span class="size-2 rounded-full bg-red-500 animate-pulse" />
              <span class="text-label-small font-medium text-red-400">{{ $t('On Now') }}</span>
            </div>
            <h3 class="text-title-medium font-semibold">
              {{ currentProgram.title }}
            </h3>
            <p v-if="currentProgram.description" class="mt-1 line-clamp-3 text-body-small opacity-60">
              {{ currentProgram.description }}
            </p>
            <div class="mt-3">
              <div class="flex justify-between text-body-small opacity-50">
                <span>{{ formatTime(currentProgram.start) }}</span>
                <span v-if="currentProgram.stop">{{ formatTime(currentProgram.stop) }}</span>
              </div>
              <div class="mt-1 h-1.5 w-full overflow-hidden rounded-full bg-white/10">
                <div
                  class="h-full w-full origin-left rounded-full bg-primary transition-transform duration-300 ease-linear"
                  :style="{ transform: `scaleX(${programProgress(currentProgram) / 100})` }"
                />
              </div>
            </div>
          </div>

          <!-- Play button -->
          <div class="px-5 py-3">
            <v-btn
              color="primary"
              block
              size="large"
              class="font-semibold"
              @click="emit('play', channel)"
            >
              {{ $t('Watch Now') }}
            </v-btn>
          </div>

          <!-- Upcoming -->
          <div class="min-h-0 flex-1 overflow-y-auto px-5 pb-5">
            <h3 v-if="pastPrograms.length" class="mb-3 text-label-large font-medium opacity-50">
              {{ $t('Catch up') }}
            </h3>
            <div v-if="pastPrograms.length" class="mb-4 space-y-1">
              <button
                v-for="p in pastPrograms"
                :key="`past-${p.start}-${p.title}`"
                type="button"
                class="flex w-full items-start gap-3 rounded-lg px-3 py-2.5 text-start transition-colors hover:bg-white/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                @click="emit('play', channel, p)"
              >
                <v-icon icon="mdiHistory" size="16" class="mt-0.5 shrink-0 text-primary/70" />
                <div class="min-w-0 flex-1">
                  <p class="line-clamp-1 text-body-medium font-medium">
                    {{ p.title }}
                  </p>
                  <p v-if="p.description" class="mt-0.5 line-clamp-2 text-body-small opacity-50">
                    {{ p.description }}
                  </p>
                </div>
                <span class="shrink-0 text-body-small opacity-40">
                  {{ formatTimeRange(p) }}
                </span>
              </button>
            </div>

            <h3 v-if="upcomingPrograms.length" class="mb-3 text-label-large font-medium opacity-50">
              {{ $t('Upcoming') }}
            </h3>
            <div v-else-if="programs.length <= 1 && !pastPrograms.length" class="py-8 text-center">
              <p class="text-body-medium opacity-40">
                {{ $t('No EPG data available') }}
              </p>
            </div>

            <div class="space-y-1">
              <div
                v-for="p in upcomingPrograms"
                :key="`${p.start}-${p.title}`"
                class="rounded-lg px-3 py-2.5 transition-colors hover:bg-white/5"
              >
                <div class="flex items-start justify-between gap-3">
                  <div class="min-w-0 flex-1">
                    <p class="line-clamp-1 text-body-medium font-medium">
                      {{ p.title }}
                    </p>
                    <p v-if="p.description" class="mt-0.5 line-clamp-2 text-body-small opacity-50">
                      {{ p.description }}
                    </p>
                  </div>
                  <span class="shrink-0 text-body-small opacity-40">
                    {{ formatTime(p.start) }}
                  </span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </transition>
  </teleport>
</template>

<style scoped>
.drawer-enter-active,
.drawer-leave-active {
  transition: opacity 200ms ease;
}
.drawer-enter-active > div:last-child,
.drawer-leave-active > div:last-child {
  transition: transform 250ms cubic-bezier(0.16, 1, 0.3, 1);
}
.drawer-enter-from,
.drawer-leave-to {
  opacity: 0;
}
.drawer-enter-from > div:last-child,
.drawer-leave-to > div:last-child {
  transform: translateX(100%);
}
</style>
