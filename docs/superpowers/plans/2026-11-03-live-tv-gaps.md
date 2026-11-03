# Live TV Gaps Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the gaps found in Live TV, Free TV and Premium TV: a working Free TV guide, recovery when a playing free stream drops, remote number tuning, Xtream catch-up, several Premium accounts, programme reminders, and the dead code around them.

**Architecture:** Every task builds on plumbing that already exists but was never connected: the `iptv_epg_programs` table and `epg_id` column, the stubbed `live_get_live_epg`/`live_channel_epg_batch` commands, Premium's `parse_xmltv_time`, the signed play-id redirector (`movie:`/`series:`), the `connection_id`-keyed Premium schema whose `connect` already keeps old accounts as `inactive`, and the shared `<live-tv-live-player-overlay>` both live players mount with `@zap-to`. Pure logic goes in `app/utils/*` so the `check:*` scripts can assert it under bun; Rust logic gets `#[cfg(test)]` unit tests.

**Tech Stack:** Nuxt 4 + Vuetify 4 + Pinia; Tauri 2; Rust (rusqlite, reqwest, axum, chrono 0.4, flate2 1); bun check scripts; `cargo test`.

**Spec:** the Live TV analysis in this session (findings 1–7). Measured on Windows, 2026-11-03: 0 of the first 60 Free TV channels had guide data, because `live_get_live_epg` and `live_channel_epg_batch` in `src-tauri/src/iptv/commands.rs` return `Ok(Vec::new())`; 1,161 of 2,814 free channels were offline; `tv_archive` arrives from Xtream but is never stored; no handler reads digit keys; `LiveEpgDrawer`/`LiveEpgSheet` and 21 other components are mounted nowhere; Ctrl/Cmd+K queries `[data-live-search]`, which exists only in the unmounted `LiveTvSearch`.

## Global Constraints

- Every on-screen string is `$t('English text')`; run `bun run i18n` after adding any; `bun run check:i18n` must pass.
- Every interactive element is a real `<button>`/`<a>`; every `hover:` has a `focus-visible:` twin; nothing depends on typing or pointing (`.claude/skills/tv-remote-ui/SKILL.md`).
- No default, example or bundled guide URL anywhere, including placeholders, comments and the README. Guide URLs are the user's own, exactly like sources.
- A Premium upstream URL or credential never reaches the front end; playback goes through `/premium-stream/:token` only. `check:premium-player`'s "UI never builds a stream URL" assertion must keep passing, and nothing in `app/` may contain `/timeshift/`.
- No new test framework: assertions go in the existing `scripts/check-*.ts`; Rust uses `#[cfg(test)]`.
- No `backdrop-filter` per list item; bulk lists keep `content-visibility: auto`.
- Comments explain why, not what.
- Done means `bun run lint`, `bun run check:types`, the touched `check:*` scripts and `cargo test --manifest-path src-tauri/Cargo.toml` all pass.

## Review Focus

1. **A 150 MB gzip XMLTV file**: the import must not freeze the UI or exhaust memory. It runs in Rust off the main thread, caps decompressed size at 256 MiB, and reports that URL as failed rather than aborting the others. *Test: Task 3, `rejects_oversized_guide`.*
2. **XMLTV channel ids that differ from the playlist's `tvg-id`** by case or by a `@SD`/`@HD` feed suffix (`1Plus1Ukraina.ua@SD` vs `1Plus1Ukraina.ua`) must still match. *Test: Task 3, `matches_epg_id_ignoring_case_and_feed_suffix`.*
3. **Catch-up at the window edges**: a programme still on air, one starting in the future, or one older than `archive_days` must be refused by the API, not by hiding a button. *Test: Task 6, `archive_window_rejects_live_future_and_expired`.*
4. **Typing digits into a search field** must not tune a channel, and `0005` or a number beyond the lineup must do nothing. *Test: Task 2, the `channelDigitKey` and `channelIndexFor` assertions.*
5. **Switching or removing the active Premium account while a channel plays** must stop playback and reload the catalogue, not keep streaming the old account's channel. *Test: Task 7, the store assertion that `switchTo` calls `resetPlayer()` and reloads.*

---

## Order and independence

Tasks 1, 2, 3, 5, 6 and 7 touch disjoint files and can land in any order. Task 4 extends Task 3's importer, and Task 8 needs Task 3's free guide data for its free-channel bell. Recommended order, by value per effort: **1 → 2 → 3 → 4 → 5 → 6 → 7 → 8**. Each task is one commit.

---

### Task 1: Delete the orphaned components and fix Ctrl/Cmd+K

**Files:**
- Delete: `app/components/live-tv/{ActiveFilterBar,CategorySelector,ConnectionStatus,CountrySelector,EmptyState,LiveCategoryBar,LiveChannelScrollRow,LiveChannelSkeleton,LiveConnectionStatus,LiveCountryBar,LiveEPG,LiveEpgDrawer,LiveEpgSheet,LiveFilterDrawer,LiveNowSection,LiveResultHeader,LiveSearch,LiveStatusBadge,LiveTvPageHeader,LiveTvPrimaryNav,LiveTvSearch,RecentSection}.vue`, `app/components/premium-tv/PremiumVodScrollRow.vue`
- Modify: `app/components/live-tv/LiveBrowseHeader.vue:74` (the `<search-field>`), `app/plugins/keyboard-shortcuts.client.ts`
- Test: `scripts/check-free-tv.ts`

**Interfaces:** Produces nothing later tasks use. Task 6 adds catch-up to `PremiumEpgPanel.vue`, not to the deleted drawer.

- [ ] **Step 1: Write the failing test** (append to `scripts/check-free-tv.ts`)

```ts
const browseHeader = readFileSync(new URL('../app/components/live-tv/LiveBrowseHeader.vue', import.meta.url), 'utf8')
assert.match(browseHeader, /<search-field[^>]*\bdata-live-search\b/, 'the mounted live search carries the Ctrl+K hook')
const shortcuts = readFileSync(new URL('../app/plugins/keyboard-shortcuts.client.ts', import.meta.url), 'utf8')
assert.match(shortcuts, /\[data-live-search\] input/, 'Ctrl+K focuses the input inside the field, not its wrapper')
for (const gone of ['LiveEpgDrawer', 'LiveEpgSheet', 'LiveTvSearch', 'LiveFilterDrawer'])
  assert.ok(!existsSync(new URL(`../app/components/live-tv/${gone}.vue`, import.meta.url)), `${gone} was never mounted`)
```
(Add `existsSync` to the file's `node:fs` import.)

- [ ] **Step 2: Run it.** `bun run check:free-tv`. Expected: FAIL on "carries the Ctrl+K hook".
- [ ] **Step 3: Implement.** Delete the 23 files. Add `data-live-search` to the `<search-field>` in `LiveBrowseHeader.vue`. In the plugin, query `[data-live-search] input` and keep the existing focus/blur toggle.
- [ ] **Step 4: Verify.** `bun run check:free-tv && bun run check:premium-player && bun run check:types && bun run i18n && bun run check:i18n`. Expected: all pass; the catalogs lose the keys only the deleted files used.
- [ ] **Step 5: Commit.** `git commit -m "chore(live-tv): drop 23 unmounted components; Ctrl+K reaches the live search"`

---

### Task 2: Number-key tuning and last-channel recall

**Files:**
- Create: `app/utils/channelDigits.ts`
- Modify: `app/components/live-tv/LivePlayerOverlay.vue` (its `onKeyDown` at ~517, its `defineEmits` at ~145, and its template), `app/pages/live-tv/watch.vue` (`zapTo` ~371), `app/pages/live-tv/premium/watch.vue` (`zapTo` handler)
- Test: `scripts/check-free-tv.ts`

**Interfaces:**
- Produces in `app/utils/channelDigits.ts`:
  - `export const DIGIT_COMMIT_MS = 1500`
  - `export function channelDigitKey(e: Pick<KeyboardEvent, 'key' | 'code' | 'target'>): string | null` returns `'0'`–`'9'` for top-row and numpad digits, and `null` when `target` is an `input`, `textarea` or `[contenteditable]`, or the key isn't a digit.
  - `export function appendDigit(buffer: string, digit: string): string` keeps at most 4 digits, dropping the oldest.
  - `export function channelIndexFor(buffer: string, count: number): number | null` maps the 1-based channel number to a 0-based index, or `null` for an empty buffer, `0`, or anything past `count`.
- The overlay emits `recall: []` alongside the existing `zapTo: [index: number]`.

- [ ] **Step 1: Write the failing test**

```ts
import { appendDigit, channelDigitKey, channelIndexFor } from '../app/utils/channelDigits'
const ev = (key: string, code = '', target: unknown = null) => ({ key, code, target }) as never
assert.equal(channelDigitKey(ev('7', 'Digit7')), '7')
assert.equal(channelDigitKey(ev('3', 'Numpad3')), '3')
assert.equal(channelDigitKey(ev('a', 'KeyA')), null)
assert.equal(channelDigitKey(ev('5', 'Digit5', { tagName: 'INPUT' })), null, 'typing in search never tunes')
assert.equal(appendDigit('1234', '5'), '2345')
assert.equal(channelIndexFor('105', 200), 104)
assert.equal(channelIndexFor('0005', 200), 4)
assert.equal(channelIndexFor('201', 200), null)
assert.equal(channelIndexFor('0', 200), null)
assert.equal(channelIndexFor('', 200), null)
const overlay = readFileSync(new URL('../app/components/live-tv/LivePlayerOverlay.vue', import.meta.url), 'utf8')
assert.match(overlay, /channelDigitKey\(/, 'the overlay reads digits')
assert.match(overlay, /emit\('recall'\)/, 'a lone 0 recalls the previous channel')
for (const page of ['watch.vue', 'premium/watch.vue'])
  assert.match(readFileSync(new URL(`../app/pages/live-tv/${page}`, import.meta.url), 'utf8'), /@recall=/, `${page} wires recall`)
```

- [ ] **Step 2: Run it.** `bun run check:free-tv`. Expected: FAIL, module not found.
- [ ] **Step 3: Implement `app/utils/channelDigits.ts`** with the signatures above. The target check reads `tagName` and `isContentEditable`, so the plain objects in the test work.
- [ ] **Step 4: Wire the overlay.** In `onKeyDown`, a digit appends to a `buffer` ref and restarts a `DIGIT_COMMIT_MS` timer. When the timer fires, a non-null `channelIndexFor(buffer, channelList.length)` emits `zapTo`, and an empty or out-of-range buffer is cleared without zapping. `0` pressed with an empty buffer emits `recall` immediately. While `buffer` is non-empty, show it in the channel-number slot (the `channelNumber` computed at ~189) as `105_`, with `aria-live="polite"`.
- [ ] **Step 5: Wire the pages.** Each page keeps `previousChannelId` (the id that was current before the last `zapTo`). `@recall` zaps to that channel's index in the current lineup; if it isn't in the lineup, it does nothing.
- [ ] **Step 6: Verify.** `bun run check:free-tv && bun run check:dpad && bun run check:premium-player && bun run check:types`. Expected: pass. Then `bun run tauri:dev`, open a Free TV channel, type `1`, `0`, `5` and wait 1.5 s: channel 105 plays. Press `0`: the previous channel plays.
- [ ] **Step 7: Commit.** `git commit -m "feat(live-tv): tune by channel number; 0 recalls the last channel"`

---

### Task 3: Free TV guide from the user's own XMLTV URLs

**Files:**
- Create: `src-tauri/src/iptv/guides.rs`
- Modify: `src-tauri/src/iptv/mod.rs` (`pub mod guides;`), `src-tauri/src/iptv/commands.rs:429-442` (replace both stubs; add three commands), `src-tauri/src/lib.rs` (register the commands; start-up import), `src-tauri/src/premium/repository.rs:1012` (`fn parse_xmltv_time` becomes `pub(crate)`), `app/utils/iptv.ts`, `app/pages/live-tv/settings.vue`
- Test: `#[cfg(test)] mod tests` in `guides.rs`; `scripts/check-free-tv.ts`

**Interfaces:**
- Produces in `src-tauri/src/iptv/guides.rs`:
  - `pub const MAX_GUIDE_URLS: usize = 10`, `pub const MAX_GUIDE_BYTES: usize = 256 * 1024 * 1024`, `pub const GUIDE_TTL_SECS: i64 = 12 * 3600`
  - `pub fn guide_urls(conn: &Connection) -> Result<Vec<String>, IptvError>` and `pub fn set_guide_urls(conn: &Connection, urls: &[String]) -> Result<Vec<String>, IptvError>`. These store JSON in `iptv_meta` under key `guide_urls`. Setting trims, drops anything not `http(s)://`, de-duplicates, caps at `MAX_GUIDE_URLS` and returns what was kept.
  - `pub fn feed_key(xmltv_channel: &str) -> String` lowercases and strips a trailing `@…` feed suffix. It's applied to both sides of the match.
  - `pub fn decode_guide(raw: &[u8]) -> Result<String, IptvError>` gunzips when the bytes start `1f 8b` and errors once past `MAX_GUIDE_BYTES`.
  - `#[derive(Serialize)] #[serde(rename_all = "camelCase")] pub struct GuideReport { pub programs: usize, pub channels: usize, pub failed: Vec<String> }`
  - `pub async fn import_guides(state: &IptvState) -> Result<GuideReport, IptvError>` fetches each URL, decodes, parses with `super::xmltv::parse_xmltv`, maps `feed_key(program.channel_id)` to `iptv_channels.id` via `feed_key(epg_id)` for `sources::FREE_TV_SOURCE_ID`, keeps the window `[now − 3 h, now + 36 h]`, converts times with `crate::premium::repository::parse_xmltv_time`, applies `guide_offset_minutes` (Task 4; 0 until then), and replaces the source's rows in `iptv_epg_programs` in one transaction. It stamps `iptv_meta.guide_imported_at`. A URL that fails goes into `failed` and the others continue.
  - `pub fn programs_for(conn: &Connection, source_id: &str, channel_ids: &[String], now: i64) -> Result<Vec<EpgProgram>, IptvError>` returns rows in `[now − 3 h, now + 36 h]`, ordered by channel then start, with `start`/`stop` as RFC 3339 (`chrono::DateTime::from_timestamp(..).to_rfc3339()`), which is the format the front end's `new Date()` reads.
- Commands (camelCase JS args): `live_get_live_epg(channel_id)` and `live_channel_epg_batch(channel_ids)` call `programs_for`; `live_guide_urls() -> Vec<String>`, `live_set_guide_urls(urls) -> Vec<String>`, `live_import_guides() -> GuideReport`.
- In `app/utils/iptv.ts`: `guideUrls(): Promise<string[]>`, `setGuideUrls(urls: string[]): Promise<string[]>`, `importGuides(): Promise<{ programs: number, channels: number, failed: string[] }>`.

- [ ] **Step 1: Write the failing Rust tests** in `guides.rs`:
  - `feed_key_strips_case_and_suffix`: `feed_key("1Plus1Ukraina.ua@SD") == "1plus1ukraina.ua"`.
  - `matches_epg_id_ignoring_case_and_feed_suffix`: an in-memory DB (the same open helper `db.rs` tests use) with a free channel whose `epg_id = "1Plus1Ukraina.ua"`, and an XMLTV string whose programme `channel="1plus1ukraina.ua@HD"` starts within the hour. After a `store_programs` helper call, `programs_for` returns it with an RFC 3339 `start`.
  - `drops_programmes_outside_window`: one programme 2 days old and one 3 days ahead are not stored.
  - `rejects_oversized_guide`: `decode_guide(&[0u8; MAX_GUIDE_BYTES + 1])` is `Err`, and a gzip of `MAX_GUIDE_BYTES + 1` bytes is `Err` without decoding all of it.
  - `set_guide_urls_keeps_only_http_and_caps`: `["ftp://x", " https://a/g.xml ", "https://a/g.xml", "javascript:1"]` keeps `["https://a/g.xml"]`.
  (Split import into `fetch` and `store_programs(conn, source_id, programs, now, offset_minutes)` so the tests need no network.)
- [ ] **Step 2: Run them.** `cargo test --manifest-path src-tauri/Cargo.toml guides`. Expected: FAIL to compile.
- [ ] **Step 3: Implement `guides.rs`, the five commands and their registration in `lib.rs`.** The start-up import runs in the existing background block after the free import, only when `guide_urls` isn't empty and `guide_imported_at` is older than `GUIDE_TTL_SECS`. Use the same `reqwest` client and timeouts as `iptv/epg.rs`.
- [ ] **Step 4: Run them.** Expected: PASS.
- [ ] **Step 5: Settings UI.** Add a `settings-section` titled `$t('Programme guide')` to `app/pages/live-tv/settings.vue`. It has: a list of URLs, each with a remove button; a URL field with an Add button (no placeholder URL); `$t('Update guide now')`, which calls `importGuides()` and then reports `$t('{programs} programmes for {channels} channels', …)` plus each failed URL; and the explainer `$t('Rivulet has no guide of its own. Add an XMLTV address (.xml or .xml.gz) from a guide you trust; channels are matched by their tvg-id.')`. All controls are real buttons with focus twins.
- [ ] **Step 6: Front-end check.** In `scripts/check-free-tv.ts`, assert that `settings.vue` calls `setGuideUrls` and `importGuides`, and that it contains no `https?://` literal. Run `bun run i18n && bun run check:free-tv && bun run check:types`.
- [ ] **Step 7: Verify in the app.** Add a real XMLTV URL you have, press *Update guide now*, open Free TV: matched channels show Now/Next.
- [ ] **Step 8: Commit.** `git commit -m "feat(free-tv): programme guide from the user's own XMLTV URLs"`

---

### Task 4: Guide time offset

**Files:** Modify `src-tauri/src/iptv/guides.rs`, `src-tauri/src/iptv/commands.rs`, `src-tauri/src/lib.rs`, `app/utils/iptv.ts`, `app/pages/live-tv/settings.vue`. Test in `guides.rs`.

**Interfaces:**
- Consumes `store_programs(conn, source_id, programs, now, offset_minutes)` from Task 3.
- Produces `pub fn guide_offset(conn) -> Result<i32, IptvError>` and `pub fn set_guide_offset(conn, minutes: i32) -> Result<i32, IptvError>`, clamped to `-720..=720` and stored in `iptv_meta.guide_offset_minutes`. Commands `live_guide_offset() -> i32` and `live_set_guide_offset(minutes) -> i32`. Front end: `guideOffset()` and `setGuideOffset(minutes)`.

- [ ] **Step 1: Failing test** `offset_shifts_start_and_stop`: storing a 12:00–13:00 programme with offset `+60` reads back as 13:00–14:00. `set_guide_offset(conn, 5000)` returns `720`.
- [ ] **Step 2: Run** `cargo test guides`. Expected: FAIL.
- [ ] **Step 3: Implement.** `import_guides` reads the offset and passes it through. Changing the offset re-runs the import.
- [ ] **Step 4: UI.** Under *Programme guide*, add `$t('Guide time correction')` with a − button, the value `±N min` and a + button, in 30-minute steps. These are buttons, not a slider, so a remote can use them.
- [ ] **Step 5: Verify** `cargo test guides && bun run i18n && bun run check:types`.
- [ ] **Step 6: Commit.** `git commit -m "feat(free-tv): correct a guide that runs early or late"`

---

### Task 5: Free TV reconnects a stream that was playing

**Files:**
- Create: `app/utils/liveRecovery.ts`
- Modify: `app/stores/premiumTv.ts:45-61` (keep `MAX_RECONNECT_ATTEMPTS`/`reconnectDelayMs` as re-exports from the util), `app/pages/live-tv/watch.vue` (`onPlaybackFailed` ~496)
- Test: `scripts/check-free-tv.ts`

**Interfaces:**
- Produces in `app/utils/liveRecovery.ts`:
  - `export const MAX_RECONNECT_ATTEMPTS = 4` and `export function reconnectDelayMs(attempt: number): number`, moved verbatim from the store.
  - `export type LiveFailureAction = 'reconnect' | 'fallback' | 'skip' | 'error'`
  - `export function nextLiveAction(s: { hadPicture: boolean, reconnects: number, canFallback: boolean, fallbackTried: boolean, autoSkips: number, maxAutoSkips: number }): LiveFailureAction`. The rules: a channel that showed a picture reconnects while `reconnects < MAX_RECONNECT_ATTEMPTS` and is never skipped away from, then becomes `error`. A channel that never showed one tries `fallback` once if `canFallback`, then `skip` while `autoSkips < maxAutoSkips`, then `error`.

- [ ] **Step 1: Failing test**

```ts
import { nextLiveAction } from '../app/utils/liveRecovery'
const base = { hadPicture: false, reconnects: 0, canFallback: true, fallbackTried: false, autoSkips: 0, maxAutoSkips: 3 }
assert.equal(nextLiveAction({ ...base, hadPicture: true }), 'reconnect', 'a dropped stream that was playing comes back')
assert.equal(nextLiveAction({ ...base, hadPicture: true, reconnects: 4 }), 'error', 'and stops after four')
assert.equal(nextLiveAction(base), 'fallback')
assert.equal(nextLiveAction({ ...base, fallbackTried: true }), 'skip')
assert.equal(nextLiveAction({ ...base, fallbackTried: true, autoSkips: 3 }), 'error')
const freeWatch = readFileSync(new URL('../app/pages/live-tv/watch.vue', import.meta.url), 'utf8')
assert.match(freeWatch, /nextLiveAction\(/, 'the free player decides through the shared policy')
assert.match(freeWatch, /reconnectDelayMs\(/, 'with the same backoff as Premium')
```

- [ ] **Step 2: Run** `bun run check:free-tv`. Expected: FAIL.
- [ ] **Step 3: Implement the util** and re-export from `premiumTv.ts`. Both `premium/watch.vue` and `scripts/check-premium-player.ts:28` import the constants from the store. `onPlaybackFailed` passes `maxAutoSkips: MAX_AUTO_SKIPS` from `app/utils/livehealth.ts` (currently 5).
- [ ] **Step 4: Rewire `onPlaybackFailed`.** Keep a `hadPicture` ref that becomes true on the first picture and resets on channel change. This is separate from `hasPicture`, which drops on restart. Add a `reconnects` ref that resets on picture. Map `reconnect` to `setTimeout(onRetry, reconnectDelayMs(n))`, and show the overlay status `$t('Reconnecting… attempt {attempt} of {total}', …)`, the same string Premium uses. Map `fallback` to the existing `.m3u8` → `.ts` block, `skip` to `autoSkip()`, and `error` to the existing message. Delete the early `if (hasPicture.value) return`, because the policy now covers it.
- [ ] **Step 5: Verify** `bun run check:free-tv && bun run check:premium-player && bun run check:types`. In the app, play a free channel and block its host with a firewall rule. Expected: "Reconnecting… attempt 1 of 4" and so on, then the error, with no zap away.
- [ ] **Step 6: Commit.** `git commit -m "fix(free-tv): a stream that was playing reconnects instead of giving up"`

---

### Task 6: Xtream catch-up

**Files:**
- Modify: `src-tauri/src/premium/xtream.rs` (`XtreamStream` ~356, `parse_vod_play_id` ~1187, `resolve_stream_url` ~774, the auth/server-info parse ~304-347), `src-tauri/src/premium/storage.rs` (`iptv_premium_channels` columns via the `migrate` ladder ~145, `replace_channels` ~451, a connection column `server_utc_offset_secs`), `src-tauri/src/premium/models.rs` (`IPTVChannel`), `src-tauri/src/premium/repository.rs` (`channel_row` ~860), `src-tauri/src/api/routes_premium.rs`, `src-tauri/src/api/mod.rs:208`, `app/types/*` (`IPTVChannel`), `app/utils/premiumTv.ts`, `app/components/premium-tv/PremiumEpgPanel.vue`, `app/pages/live-tv/premium/watch.vue`
- Test: Rust `#[cfg(test)]` in `xtream.rs` and `routes_premium.rs`; `scripts/check-premium-player.ts`

**Interfaces:**
- Rust: `XtreamStream` gains `tv_archive` and `tv_archive_duration`, each `Option<serde_json::Value>` because panels send numbers or strings. `fn archive_days(s: &XtreamStream) -> Option<i64>` returns `Some(d)` only when `tv_archive` is truthy and `d > 0`.
- `IPTVChannel.archive_days: Option<i64>` is serialised as `archiveDays`, stored in the new `archive_days INTEGER` column, and is `None` for M3U accounts.
- Play id `archive:{stream_id}:{start_unix}:{minutes}`. `parse_vod_play_id` returns `("archive", stream_id, "{start_unix}:{minutes}")`.
- `pub fn timeshift_url(server: &str, user: &str, pass: &str, stream_id: &str, start_unix: i64, minutes: i64, server_offset_secs: i64) -> String` produces `{server}/timeshift/{user}/{pass}/{minutes}/{YYYY-MM-DD:HH-MM}/{stream_id}.ts`. The time is formatted in the panel's local time (`start_unix + server_offset_secs`) and user and password are URL-encoded. The offset is taken at connect time from `server_info`: `time_now` (naive local) minus `timestamp_now`, and is 0 when either is missing.
- `pub fn archive_window_ok(archive_days: i64, start_unix: i64, end_unix: i64, now: i64) -> bool` requires `end_unix <= now`, `start_unix >= now - archive_days * 86_400` and `end_unix > start_unix`.
- Route `POST /api/premium-tv/channels/:id/archive` takes a body `{ start: i64, end: i64 }`, returns `PlaybackSource` (the same as `/play`), and answers 422 when `archive_window_ok` fails or the channel has no `archive_days`. The play ID's `minutes` is `(end - start + 59) / 60`, capped at 360.
- Front end: `premiumApi.archive(channelId: string, start: number, end: number): Promise<PlaybackSource>`. The watch page accepts `?archive={start}-{end}`.

- [ ] **Step 1: Failing Rust tests**
  - `archive_days_reads_numbers_and_strings`: `"tv_archive":1,"tv_archive_duration":7` gives `Some(7)`; `"tv_archive":"1","tv_archive_duration":"3"` gives `Some(3)`; `"tv_archive":0` gives `None`.
  - `timeshift_url_formats_panel_local_time`: start `1704110400` (2024-01-01 12:00 UTC) with offset `3600` yields `…/timeshift/u%40x/p/60/2024-01-01:13-00/55.ts` for user `u@x`.
  - `archive_window_rejects_live_future_and_expired`: with `now = 1_000_000` and 7 days, refuse end > now, refuse start < now − 7 d, accept a programme that ended 1 h ago.
  - `play_id_round_trips_archive`: `parse_vod_play_id("archive:55:1704110400:60") == Some(("archive","55","1704110400:60"))`.
- [ ] **Step 2: Run** `cargo test premium`. Expected: FAIL to compile.
- [ ] **Step 3: Implement the Rust side.** Add the `migrate` ladder entries `iptv_premium_channels.archive_days INTEGER` and `iptv_premium_connections.server_utc_offset_secs INTEGER`. Add the `resolve_stream_url` branch for `archive`, plus the route and its registration next to `/play`. The 422 body uses the existing `ApiError` shape.
- [ ] **Step 4: Run** `cargo test premium`. Expected: PASS.
- [ ] **Step 5: Failing front-end check** in `scripts/check-premium-player.ts`. Add `globSync` to its `node:fs` import.

```ts
check('catch-up goes through the API, never a URL', () => {
  const panel = read(`${ROOT}app/components/premium-tv/PremiumEpgPanel.vue`)
  assert.ok(panel.includes('archiveDays'), 'only channels that keep an archive offer it')
  assert.ok(/premiumApi\.archive\(|emit\('watch-from-start'/.test(panel), 'the panel asks the API')
  for (const f of globSync('app/**/*.{vue,ts}', { cwd: ROOT }))
    assert.ok(!read(`${ROOT}${f}`).includes('/timeshift/'), `${f} must not build a timeshift URL`)
})
```

- [ ] **Step 6: Implement the UI.** `PremiumEpgPanel` shows past programmes, newest first. When the channel has `archiveDays` and `archive_window_ok` would pass (the same rule, recomputed client-side for display only), each has a `$t('Watch from start')` button. It emits `watch-from-start` with `{ start, end }`. The watch page routes to `?archive=start-end`, requests `premiumApi.archive`, and puts the overlay into its on-demand mode, so the clock and seeking work. A 422 shows `$t('This programme is no longer in the archive.')`.
- [ ] **Step 7: Verify** `bun run check:premium-player && bun run check:premium-api && bun run check:types && bun run i18n`. With an Xtream account whose channels have catch-up, *Watch from start* plays the past programme from its beginning.
- [ ] **Step 8: Commit.** `git commit -m "feat(premium-tv): catch-up on Xtream channels that keep an archive"`

---

### Task 7: Several Premium accounts

**Files:**
- Modify: `src-tauri/src/premium/storage.rs` (near `insert_connection` ~270 and `active_connection` ~355), `src-tauri/src/api/routes_premium.rs`, `src-tauri/src/api/mod.rs:196-197`, `app/utils/premiumTv.ts`, `app/stores/premiumTv.ts`, `app/pages/settings/premium-tv.vue`, `app/components/premium-tv/PremiumAccountCard.vue`
- Test: Rust `#[cfg(test)]` in `storage.rs`; `scripts/check-premium-api.ts`

**Interfaces:**
- Rust:
  - `#[serde(rename_all = "camelCase")] pub struct ConnectionSummary { pub id: String, pub provider_type: String, pub display_name: String, pub account_name: Option<String>, pub expires_at: Option<String>, pub active: bool }`
  - `pub fn list_connections(conn) -> Result<Vec<ConnectionSummary>, PremiumError>` lists the active account first, then the rest by `activated_at` descending.
  - `pub fn activate_connection(conn, id: &str) -> Result<(), PremiumError>` sets the flag in one transaction and returns `ProviderNotConnected` for an unknown id.
  - `pub fn drop_duplicate_connections(conn, keep_id: &str) -> Result<usize, PremiumError>` deletes older rows with the same `provider_type`, `display_name` and `account_name`. Without it, reconnecting the same login piles up copies.
- Routes: `GET /api/premium-tv/connections`, `POST /api/premium-tv/connections/:id/activate`, `DELETE /api/premium-tv/connections/:id`. The delete calls `vod_cache.clear_connection(id)` and has no entitlement gate, matching `disconnect`.
- Front end: `premiumApi.connections()`, `.activate(id)`, `.removeConnection(id)`. The store gains `connections: Ref<ConnectionSummary[]>`, `loadConnections()`, `switchTo(id)` (calls `resetPlayer()`, activates, then reloads status, channels and favourites) and `removeConnection(id)` (when the id is active, the next account becomes active, or the store returns to *not connected*).

- [ ] **Step 1: Failing Rust tests**
  - `list_puts_active_first_and_activate_switches`: insert A then B (B active), then `activate_connection(A)`. The list is `[A(active), B]`.
  - `activate_unknown_is_not_connected`.
  - `duplicates_are_dropped_on_reconnect`: two rows with an identical display and account name, then `drop_duplicate_connections(newer)`, leaves one row.
- [ ] **Step 2: Run** `cargo test storage`. Expected: FAIL.
- [ ] **Step 3: Implement** the storage functions, the routes, the call to `drop_duplicate_connections` at the end of a successful `connect`, and the route registration.
- [ ] **Step 4: Run** `cargo test storage`. Expected: PASS.
- [ ] **Step 5: Failing front-end check** in `scripts/check-premium-api.ts`: the store source contains `function switchTo`, and its body calls `resetPlayer()` before the `activate` request. `settings/premium-tv.vue` renders `connections` with *Switch* and *Remove* buttons and an `$t('Add another account')` button that opens `PremiumConnectForm` without calling `disconnect`.
- [ ] **Step 6: Implement the UI.** The account list goes on Settings → Premium TV, one `PremiumAccountCard` each. *Remove* asks for confirmation in a `v-dialog`, with real buttons and Back meaning Cancel.
- [ ] **Step 7: Verify** `bun run check:premium-api && bun run check:premium-player && bun run check:types && bun run i18n`. In the app, connect two accounts and switch while a channel plays. Expected: playback stops and the other account's channels load.
- [ ] **Step 8: Commit.** `git commit -m "feat(premium-tv): keep several accounts and switch between them"`

---

### Task 8: Programme reminders

**Files:**
- Create: `app/utils/reminders.ts`, `app/plugins/reminders.client.ts`
- Modify: `app/components/premium-tv/PremiumEpgPanel.vue`, `app/components/live-tv/LiveGuideGrid.vue` (future programmes get a bell)
- Test: `scripts/check-free-tv.ts`

**Interfaces:**
- `export interface Reminder { key: string, kind: 'free' | 'premium', channelId: string, channelName: string, title: string, start: number }` where `key = \`${kind}:${channelId}:${start}\``.
- `export const REMINDER_LEAD_MS = 60_000`
- `export function dueReminders(list: Reminder[], now: number): Reminder[]` returns those with `start - REMINDER_LEAD_MS <= now < start + 5 * 60_000`.
- `export function pruneReminders(list: Reminder[], now: number): Reminder[]` drops any more than 5 minutes past its start.
- Storage is `useLocalStorage('rivulet.liveTv.reminders', [])`, so backups carry it automatically.
- The plugin checks every 30 s while the app runs. A due reminder raises an OS notification through `@tauri-apps/plugin-notification` (already permitted as `notification:default`), plus an in-app `v-snackbar` with a `$t('Watch')` button that routes to the right watch page. It then removes the reminder.

- [ ] **Step 1: Failing test**

```ts
import { dueReminders, pruneReminders } from '../app/utils/reminders'
const r = (start: number) => ({ key: `free:c:${start}`, kind: 'free' as const, channelId: 'c', channelName: 'C', title: 'T', start })
assert.deepEqual(dueReminders([r(100_000)], 40_000).length, 1, 'due a minute ahead')
assert.deepEqual(dueReminders([r(100_000)], 30_000).length, 0, 'not before')
assert.deepEqual(pruneReminders([r(100_000)], 100_000 + 6 * 60_000).length, 0, 'gone once well past')
```

- [ ] **Step 2: Run** `bun run check:free-tv`. Expected: FAIL.
- [ ] **Step 3: Implement** the util, the plugin, and the bell buttons. Each bell is a real `<button>` with `aria-pressed`. The settings copy says `$t('Reminders ring only while Rivulet is open.')`, which is true on Android too, since a backgrounded app is frozen.
- [ ] **Step 4: Verify** `bun run check:free-tv && bun run check:dpad && bun run check:types && bun run i18n`. In the app, set a reminder for a programme starting in 2 minutes. Expected: a notification and a snackbar about a minute before it starts.
- [ ] **Step 5: Commit.** `git commit -m "feat(live-tv): remind me when a programme starts"`

---

## Not in this plan, on purpose

- **Recording.** It needs a long-running ffmpeg capture, its own disk budget, and an answer for Android, where the process is frozen in the background. It's a separate project.
- **Moving the Android master key to EncryptedSharedPreferences.** This is the TODO in `premium/crypto.rs`, but it's a Kotlin bridge change that can be tested only on a device. It's a separate project.
- **Bundling a guide or playlist URL.** The Global Constraints forbid it.
