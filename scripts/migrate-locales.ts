import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const localesDir = join(import.meta.dirname, '../i18n/locales')
const files = readdirSync(localesDir).filter(f => f.endsWith('.ts'))

let totalReplacements = 0

for (const file of files) {
  const filePath = join(localesDir, file)
  const content = readFileSync(filePath, 'utf8')

  // Replace Rivulet -> Rivulet, rivulet:// -> rivulet://, rivulet. -> rivulet.
  const updated = content
    .replace(/Rivulet/g, 'Rivulet')
    .replace(/rivulet:\/\//g, 'rivulet://')
    .replace(/rivulet\./g, 'rivulet.')

  if (updated !== content) {
    writeFileSync(filePath, updated, 'utf8')
    totalReplacements++
  }
}

console.log(`Migrated ${totalReplacements} locale files to Rivulet.`)
