// Self-check for brand consistency across files that cannot import app/brand.ts
import assert from 'node:assert'
import { readFileSync } from 'node:fs'
import { APP_NAME, APP_SCHEME, BUNDLE_ID, STORAGE_PREFIX } from '../app/brand'

assert.equal(APP_NAME, 'Rivulet', 'APP_NAME is Rivulet')
assert.equal(APP_SCHEME, 'rivulet', 'APP_SCHEME is rivulet')
assert.equal(STORAGE_PREFIX, 'rivulet.', 'STORAGE_PREFIX is rivulet.')
assert.match(BUNDLE_ID, /^io\.github\..+\.Rivulet$/, 'BUNDLE_ID matches io.github.<owner>.Rivulet')

const tauri = JSON.parse(readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'))
assert.equal(tauri.productName, APP_NAME, 'tauri.conf.json productName matches APP_NAME')
assert.equal(tauri.identifier, BUNDLE_ID.toLowerCase(), 'tauri.conf.json identifier matches BUNDLE_ID lowercased')
assert.ok(
  tauri.plugins?.['deep-link']?.desktop?.schemes?.includes(APP_SCHEME),
  'tauri.conf.json deep-link includes APP_SCHEME',
)

const cargo = readFileSync(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8')
assert.match(cargo, new RegExp(`^\\[package\\]\\s*\\nname\\s*=\\s*"${APP_SCHEME}"`, 'm'), 'Cargo.toml package name matches APP_SCHEME')

const strings = readFileSync(
  new URL('../src-tauri/gen/android/app/src/main/res/values/strings.xml', import.meta.url),
  'utf8',
)
assert.match(strings, new RegExp(`<string name="app_name">${APP_NAME}</string>`), 'Android strings.xml app_name matches APP_NAME')

const boot = readFileSync(new URL('../app/boot-diagnostics.js', import.meta.url), 'utf8')
assert.match(boot, new RegExp(`${STORAGE_PREFIX}ground`), 'boot-diagnostics.js uses STORAGE_PREFIX ground')
assert.match(boot, new RegExp(`${APP_NAME.toUpperCase()}`), 'boot-diagnostics.js contains uppercase APP_NAME')

// eslint-disable-next-line no-console
console.log('brand check ok')
