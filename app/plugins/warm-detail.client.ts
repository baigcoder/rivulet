/**
 * Title, person, and season routes are separate Vite chunks. Import them with
 * the app so the first click is not an empty chrome while the chunk lands.
 */
import '~/pages/[type]/[id].vue'
import '~/pages/people/[id].vue'
import '~/pages/tv/[id]/season/[season]/index.vue'
import '~/pages/tv/[id]/season/[season]/episode/[episode].vue'
import '~/components/MediaDetailView.vue'

export default defineNuxtPlugin(() => {})
