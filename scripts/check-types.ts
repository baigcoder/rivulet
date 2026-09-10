/**
 * `vue-tsc --noEmit`, but only after proving it is actually reading our
 * `.vue` files.
 *
 * A whole-app type check that silently covers nothing is worse than no
 * check, because it reports success. This repo shipped
 * `downloads.titleFor(...)` and `containingFolder(...)` — neither of which
 * existed anywhere — past a green `check:types`, and the Play and Open
 * folder buttons both did nothing as a result.
 *
 * The cause was not a broken TypeScript. It was coverage: `--listFiles`
 * showed 58 of our `.ts` files in the program and NONE of our `.vue`
 * files, so every page and component went unchecked while the script
 * exited 0. The tsconfig include is right; vue-tsc is not expanding that
 * glob to `.vue`.
 *
 * So this asserts coverage first and fails loudly when it is missing. The
 * numbers below are floors, not targets — they only have to be high
 * enough that "checked nothing" cannot pass.
 */
import { spawnSync } from 'node:child_process'
import process from 'node:process'

/**
 * Run vue-tsc under node where there is one, else bun.
 *
 * Not a style preference. vue-tsc works by patching TypeScript's `tsc.js`
 * as it is required, and that is exactly the kind of thing two runtimes
 * can disagree about — which is the leading theory for why the program
 * below comes out with no `.vue` files in it. Unproven: the machine this
 * was written on has no node to compare against. If coverage is fine
 * under node and zero under bun, this line is the fix and the theory was
 * right.
 */
function runner(): [string, string[]] {
  const hasNode = spawnSync('node', ['--version'], { stdio: 'ignore' }).status === 0
  return hasNode ? ['node', ['node_modules/vue-tsc/bin/vue-tsc.js']] : ['bun', ['x', 'vue-tsc']]
}

const [cmd, base] = runner()

/** Our own `.vue` files that must appear in the program. */
const MIN_VUE = 40
/** Our own `.ts` files that must appear in the program. */
const MIN_TS = 40

function die(message: string): never {
  console.error(`\n✗ ${message}\n`)
  process.exit(1)
}

const listed = spawnSync(cmd, [...base, '--noEmit', '--listFiles'], {
  encoding: 'utf8',
  maxBuffer: 64 * 1024 * 1024,
})
if (listed.status == null)
  die('vue-tsc could not be run at all.')

// Only ours. node_modules carries plenty of both and would mask the gap.
const ours = (listed.stdout ?? '')
  .split('\n')
  .map(l => l.trim().split('\\').join('/'))
  .filter(l => l.includes('/app/') && !l.includes('/node_modules/'))
const vue = ours.filter(l => l.endsWith('.vue')).length
const ts = ours.filter(l => l.endsWith('.ts')).length

if (vue < MIN_VUE) {
  die(
    `vue-tsc has only ${vue} of our .vue files in its program (expected at least ${MIN_VUE}).\n`
    + '  Every page and component is going unchecked while this script exits 0 —\n'
    + '  which is how two calls to functions that did not exist reached a release.\n'
    + '  Do not "fix" this by lowering the floor. Either vue-tsc is not expanding\n'
    + '  the tsconfig include to .vue, or the generated .nuxt/tsconfig.json moved.\n'
    + '  Regenerate the types first:\n'
    + '    bun nuxt prepare',
  )
}
if (ts < MIN_TS) {
  die(
    `vue-tsc has only ${ts} of our .ts files in its program (expected at least ${MIN_TS}).\n`
    + '    bun nuxt prepare',
  )
}

console.log(`type check covering ${vue} .vue and ${ts} .ts files of our own`)
const run = spawnSync(cmd, [...base, '--noEmit'], { stdio: 'inherit' })
process.exit(run.status ?? 1)
