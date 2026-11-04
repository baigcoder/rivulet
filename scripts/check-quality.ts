import assert from 'node:assert'
import { vodVersions } from '../app/utils/providerTitle'
import { asTier, atTier, sortReleases, tierOf, tierOfHeight, tierOfText, tiersIn } from '../app/utils/quality'
// Self-check for resolution parsing and release sorting: `bun scripts/check-quality.ts`.
// The names are the shapes sources really send — scene names, addon labels,
// provider stamps — so a regression here is a menu offering the wrong thing.

const cases: [string, string][] = [
  ['Sintel.2010.2160p.BluRay.x265', '2160'],
  ['Sintel 2010 4K HDR DV', '2160'],
  ['Example\n4k DV | HDR', '2160'],
  ['Sintel.2010.UHD.BluRay', '2160'],
  ['Sintel 2010 4KHDR', '2160'],
  ['Sintel (3840x2160)', '2160'],
  ['Sintel.2010.1440p.WEB', '1440'],
  ['Sintel_2010_1080p_x264', '1080'],
  ['Sintel 2010 1080i HDTV', '1080'],
  ['Sintel FHD', '1080'],
  ['Sintel Full HD', '1080'],
  ['Sintel 1920x800 BluRay', '1080'],
  ['Sintel.2010.720p.BRrip', '720'],
  ['Sintel [720]', '720'],
  ['Sintel HD', '720'],
  ['Sintel.2010.576p.DVD', '480'],
  ['Sintel.2010.480p.WEB', '480'],
  ['Sintel 2010 SD', '480'],
  ['Sintel.2010.DVDRip.XviD', '480'],
  ['Sintel 360p', '480'],
  // A year, a runtime, an HDR or an HDTV are not a resolution.
  ['Sintel 1080 minutes', ''],
  ['Sintel.2010.HDR.WEB', ''],
  ['Sintel.2010.HDTV.x264', ''],
  ['Sintel.2010.HDRip', ''],
  ['Sintel 2010', ''],
  ['Sintel 2010 1080 x264', ''],
  ['', ''],
]
for (const [name, want] of cases)
  assert.equal(tierOfText(name), want, `"${name}" is ${want || 'unknown'}`)

// Label first, then name, then the file inside a pack.
assert.equal(tierOf({ quality: '720p', name: 'Sintel 1080p' }), '720')
assert.equal(tierOf({ quality: '', name: 'Sintel', file: 'Sintel.S01E01.1080p.mkv' }), '1080')
assert.equal(tierOf({ name: 'Sintel' }), '')

assert.equal(tierOfHeight(2160), '2160')
assert.equal(tierOfHeight(800), '1080', 'cropped scope 1080p is still 1080p')
assert.equal(tierOfHeight(720), '720')
assert.equal(tierOfHeight(576), '480')
assert.equal(tierOfHeight(0), '')

const r = (name: string, bytes: number, seeders: number, url = '') => ({ name, quality: '', bytes, seeders, url })
const list = [
  r('A 720p', 1, 50),
  r('B 2160p', 9, 10),
  r('C', 3, 500),
  r('D 1080p', 0, 20),
  r('E 1080p', 4, 90),
  r('F 480p', 2, 0, 'https://cdn.example/f.mkv'),
]
const names = (l: typeof list) => l.map(x => x.name[0]).join('')
assert.equal(names(sortReleases(list, 'quality-desc')), 'BEDAFC', 'high to low, unknown last')
assert.equal(names(sortReleases(list, 'quality-asc')), 'FAEDBC', 'low to high, unknown still last')
assert.equal(names(sortReleases(list, 'size-desc')), 'BECFAD', 'unreported size last')
assert.equal(names(sortReleases(list, 'size-asc')), 'AFCEBD')
assert.equal(names(sortReleases(list, 'seeders')), 'CEADBF', 'a link has no swarm and sorts after the torrents')
assert.equal(names(list), 'ABCDEF', 'sorting copies, never reorders the source list')

assert.deepEqual(tiersIn(list), ['2160', '1080', '720', '480'], 'only what is there, highest first')
assert.deepEqual(tiersIn([r('X', 1, 1)]), [], 'nothing parsed, nothing offered')

assert.equal(names(atTier(list, '1080')), 'DE')
assert.equal(names(atTier(list, '1440')), 'ABCDEF', 'a tier nobody has falls back to everything')
assert.equal(names(atTier(list, '')), 'ABCDEF')

assert.equal(asTier('1080'), '1080')
assert.equal(asTier('999'), '')
assert.equal(asTier(undefined), '')

// Premium: the same film listed at several qualities is the only choice an
// Xtream VOD has. Another film, or another year's remake, is not a version.
{
  const panel = [
    { id: '1', name: 'EN: Dune (2021) | FHD' },
    { id: '2', name: 'EN: Dune (2021) | 4K' },
    { id: '3', name: 'Dune (1984) | HD' },
    { id: '4', name: 'Dune: Part Two (2024) | 4K' },
    { id: '5', name: 'EN: Dune (2021) [FHD]' },
    { id: '6', name: 'EN: Dune (2021)' },
  ]
  const v = vodVersions(panel, 'EN: Dune (2021) | FHD')
  assert.deepEqual(v.map(x => [x.item.id, x.label]), [['2', '4K'], ['1', '1080p'], ['6', '']], 'highest first, one per tier, unstamped last')
  assert.deepEqual(vodVersions(panel, 'Dune: Part Two (2024) | 4K'), [], 'one listing is no choice')
  assert.deepEqual(vodVersions(panel, ''), [])
}

console.log('quality: ok')
