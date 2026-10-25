/**
 * Why a white screen is never an acceptable failure.
 *
 * The app is one ES module built for Chrome 111 (Vite's default `build.target`,
 * and Vuetify calls `Array.prototype.toSorted` besides). A webview older than
 * that cannot parse it, and a script that never parses paints nothing — so what
 * is left on screen is the activity window behind a transparent webview, and
 * "it opens to a white screen" is the entire bug report anyone can give us.
 *
 * This is inlined into the document head ahead of the bundle, in the oldest
 * JavaScript there is, so that it still runs on the webview that couldn't run
 * the app. It turns that white screen into one you can photograph: the webview's
 * version, which of the features the bundle needs are missing, and whatever was
 * actually thrown.
 *
 * It only takes the screen when nothing mounted. Once Nuxt is up, Nuxt's own
 * error page says more than this could, so errors are only collected onto
 * `window.__rivuletBoot` — which is also how to read them over adb or devtools,
 * along with `__rivuletBoot.show()` to raise the panel by hand.
 *
 * Two things happen before any of that, because the same blank screen is also
 * what a *slow* start looks like and the reports are identical. It puts the
 * colour the app last painted back straight away, so a cold start is never a
 * flash of the platform's white; and after a couple of seconds it says the app
 * is starting, with an ellipsis that moves. A moving ellipsis is worth more than
 * it looks: it is the difference between "the webview is running our code and
 * the bundle is slow" and "nothing here runs at all", which is otherwise the one
 * distinction a photograph of a dark screen cannot make.
 */
;(function () {
  // Head tags survive hydration, but a second copy would cost nothing to guard.
  if (window.__rivuletBoot)
    return

  var VERSION = '__VERSION__'
  var NEEDS = 111

  /** The default theme's ground — GROUND in app/theme/themes.ts. */
  var GROUND = '__GROUND__'

  /** Long enough that a healthy boot is never seen; short enough to beat a shrug. */
  var HINT_MS = 2500

  /** And how long before we stop being reassuring and start being useful. */
  var PANEL_MS = 12000

  var boot = window.__rivuletBoot = {
    version: VERSION,
    errors: [],
    missing: [],
    show: show,
    report: report,
  }

  /**
   * The colour to sit on until the app has one of its own.
   *
   * The build pins `html` to the default theme's ground (see `ground` in
   * nuxt.config), which is right on a first launch and wrong for anyone who has
   * since chosen a light theme — they would get a dark flash where a Windows
   * user used to get a white one. So the app writes what it actually painted to
   * `rivulet.ground` (app.vue), and this puts it back before the first frame.
   *
   * localStorage throws rather than returning null in a webview with site data
   * switched off, so this is the first of several try/catches around it.
   */
  function ground() {
    try {
      return localStorage.getItem('rivulet.ground') || GROUND
    }
    catch (e) {
      return GROUND
    }
  }

  var GROUND_NOW = ground()

  if (document.documentElement)
    document.documentElement.style.backgroundColor = GROUND_NOW

  /**
   * Is that ground a light one? Perceived brightness, not WCAG — the only thing
   * it decides is whether the two lines drawn on it are dark or pale, and the
   * app's own contrast checking (scripts/check-theme.ts) covers everything real.
   */
  function pale(hex) {
    var h = String(hex).replace('#', '')
    if (h.length === 3)
      h = h.charAt(0) + h.charAt(0) + h.charAt(1) + h.charAt(1) + h.charAt(2) + h.charAt(2)
    var n = parseInt(h, 16)
    if (h.length !== 6 || isNaN(n))
      return false
    return ((n >> 16 & 255) * 299 + (n >> 8 & 255) * 587 + (n & 255) * 114) / 1000 > 140
  }

  /**
   * What the bundle assumes, and the Chrome that first shipped it. Syntax is
   * tested through `new Function` because a webview that can't parse a feature
   * throws where it is *written*, not where it runs — which is exactly why the
   * app dies silently and this file can't use any of it.
   */
  function syntax(src) {
    try {
      new Function(src)
      return true
    }
    catch (e) {
      return false
    }
  }

  var CHECKS = [
    ['optional chaining', 80, function () { return syntax('a?.b') }],
    ['logical assignment', 85, function () { return syntax('var a; a ??= 1') }],
    ['Array.at', 92, function () { return !![].at }],
    ['Object.hasOwn', 93, function () { return !!Object.hasOwn }],
    ['Array.findLast', 97, function () { return !![].findLast }],
    ['structuredClone', 98, function () { return typeof structuredClone === 'function' }],
    ['CSS @layer', 99, function () { return typeof CSSLayerBlockRule !== 'undefined' }],
    ['AbortSignal.timeout', 103, function () { return !!(window.AbortSignal && AbortSignal.timeout) }],
    ['CSS :has()', 105, function () { return !!(window.CSS && CSS.supports && CSS.supports('selector(:has(*))')) }],
    ['Array.toSorted', 110, function () { return !![].toSorted }],
  ]

  for (var i = 0; i < CHECKS.length; i++) {
    var ok = false
    try {
      ok = CHECKS[i][2]()
    }
    catch (e) {}
    if (!ok)
      boot.missing.push(CHECKS[i][0] + ' (Chrome ' + CHECKS[i][1] + ')')
  }

  function chromeVersion() {
    var m = /Chrome\/(\d+)/.exec(navigator.userAgent)
    return m ? Number(m[1]) : 0
  }

  function attempt(fn) {
    try {
      return String(fn())
    }
    catch (e) {
      return 'error'
    }
  }

  function environment() {
    var bridge = window.RivuletScreen
    return [
      ['Rivulet', VERSION],
      ['Webview', chromeVersion() ? 'Chrome ' + chromeVersion() : 'unknown'],
      ['User agent', navigator.userAgent],
      ['Page', String(location.href)],
      ['Screen', screen.width + '×' + screen.height + ' @' + (window.devicePixelRatio || 1)
        + ', viewport ' + window.innerWidth + '×' + window.innerHeight],
      ['Android bridge', bridge ? 'yes, tv=' + attempt(function () { return bridge.tv() }) : 'no'],
      ['Storage', attempt(function () {
        localStorage.setItem('rivulet.boot', '1')
        localStorage.removeItem('rivulet.boot')
        return 'ok'
      })],
      ['Network', navigator.onLine ? 'online' : 'offline'],
      ['Missing features', boot.missing.length ? boot.missing.join(', ') : 'none'],
    ]
  }

  /**
   * The one sentence worth reading from across a room. An old webview is the
   * likely answer and the only one the user can act on, so it gets said plainly;
   * anything else falls back to whatever was thrown.
   */
  function verdict() {
    var v = chromeVersion()
    if (v && v < NEEDS) {
      return 'This device\'s Android System WebView is Chrome ' + v + '. Rivulet needs '
        + NEEDS + ' or newer. Update "Android System WebView" (and Chrome) from the '
        + 'Play Store, then reopen Rivulet.'
    }
    if (boot.missing.length)
      return 'This webview is missing features Rivulet is built with: ' + boot.missing.join(', ') + '.'
    if (boot.errors.length)
      return 'The app failed to start. The error below is what stopped it.'
    return 'The app never finished starting, and nothing reported an error. The details below are all we have.'
  }

  function record(kind, message, where) {
    boot.errors.push({ kind: kind, message: String(message), where: where || '' })
    // Straight past the reassuring stage, but not instantly: an error early in
    // boot doesn't always stop the app, and one that didn't must not take the
    // screen off a page that is about to mount.
    setTimeout(escalate, 1200)
  }

  // Capture phase, because a <script> or stylesheet that fails to load fires its
  // error at the element and never reaches window in the bubble phase — and a
  // chunk that didn't arrive looks exactly like one that didn't parse.
  window.addEventListener('error', function (e) {
    if (e && e.message) {
      var at = (e.filename || '') + (e.lineno ? ':' + e.lineno + ':' + e.colno : '')
      var stack = e.error && e.error.stack ? String(e.error.stack).split('\n').slice(0, 4).join('\n') : ''
      record('error', e.message, at + (stack ? '\n' + stack : ''))
    }
    else if (e && e.target && (e.target.src || e.target.href)) {
      record('load', 'Failed to load ' + (e.target.src || e.target.href))
    }
  }, true)

  window.addEventListener('unhandledrejection', function (e) {
    var r = e && e.reason
    record('promise', (r && (r.message || r)) || 'unhandled rejection', r && r.stack ? String(r.stack).split('\n').slice(0, 4).join('\n') : '')
  })

  function mounted() {
    var root = document.getElementById('__nuxt')
    return !!(root && root.firstChild)
  }

  function report() {
    var lines = ['Rivulet failed to start', '', verdict(), '']
    var env = environment()
    for (var i = 0; i < env.length; i++)
      lines.push(env[i][0] + ': ' + env[i][1])
    for (var j = 0; j < boot.errors.length; j++) {
      var e = boot.errors[j]
      lines.push('', '[' + e.kind + '] ' + e.message, e.where)
    }
    return lines.join('\n')
  }

  function escape(text) {
    return String(text).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
  }

  var panel = null

  function show() {
    if (panel || !document.body) {
      if (!panel)
        setTimeout(show, 200)
      return
    }

    // `__rivuletBoot.show()` can raise this by hand at any moment, including
    // while the reassuring version of the same screen is still up.
    unhint()

    var html = '<h1 style="margin:0 0 12px;font-size:22px;color:#fff">Rivulet couldn\'t start</h1>'
      + '<p style="margin:0 0 20px;font-size:17px;line-height:1.5;color:#ffd280">' + escape(verdict()) + '</p>'

    var env = environment()
    html += '<table style="border-collapse:collapse;width:100%">'
    for (var i = 0; i < env.length; i++) {
      html += '<tr><td style="padding:2px 12px 2px 0;color:#8a8a8a;white-space:nowrap;vertical-align:top">'
        + escape(env[i][0]) + '</td><td style="padding:2px 0;word-break:break-all">'
        + escape(env[i][1]) + '</td></tr>'
    }
    html += '</table>'

    for (var j = 0; j < boot.errors.length; j++) {
      var e = boot.errors[j]
      html += '<pre style="margin:16px 0 0;padding:10px;background:#000;border-left:3px solid #c33;'
        + 'white-space:pre-wrap;word-break:break-all;color:#ff9a9a">' + escape(e.message)
        + (e.where ? '\n<span style="color:#8a8a8a">' + escape(e.where) + '</span>' : '') + '</pre>'
    }

    html += '<p style="margin:20px 0 0;color:#8a8a8a">Please photograph this screen and add it to '
      + 'github.com/baigcoder/rivulet/issues</p>'

    panel = document.createElement('div')
    panel.id = 'rivulet-boot-error'
    panel.setAttribute('style', 'position:fixed;top:0;left:0;right:0;bottom:0;z-index:2147483647;'
      + 'overflow:auto;background:#101010;color:#e6e6e6;padding:24px;'
      + 'font:14px/1.45 monospace;text-align:left;-webkit-text-size-adjust:none')
    panel.innerHTML = html
    document.body.appendChild(panel)

    // There is no pointer on a television and no d-pad handler either — that
    // ships in the bundle that just failed — so the arrows have to scroll this
    // by hand or the bottom of it is unreachable.
    document.addEventListener('keydown', function (event) {
      var step = event.keyCode === 40 ? 80 : event.keyCode === 38 ? -80 : 0
      if (step && panel) {
        panel.scrollTop += step
        event.preventDefault()
      }
    })
  }

  function hide() {
    if (panel && panel.parentNode) {
      panel.parentNode.removeChild(panel)
      panel = null
    }
  }

  var hint = null
  var dots = null
  var step = 0

  /**
   * "Starting…", on the app's own ground, while there is still every reason to
   * think it will start.
   *
   * This is the screen the bug reports were about: a phone that takes its time
   * over the bundle looks exactly like one that will never finish, and someone
   * looking at an unlit rectangle closes the app rather than waiting twelve
   * seconds for a diagnostic they have no reason to expect. Built out of
   * elements rather than one lump of innerHTML because the ellipsis below has to
   * hold a reference to the line it rewrites.
   */
  /**
   * The logo, glowing, over a light that breathes, with a bar that runs under
   * it — a launch screen rather than a line of text. Every moving part is
   * `transform` or `opacity`, the two things a TV's GPU animates without
   * repainting, and all of it is plain CSS keyframes, which every webview old
   * enough to fail the bundle still has. The ellipsis stays JavaScript: it is
   * the proof that our code runs, so it must not be something CSS could do on
   * its own.
   */
  var BRAND = '#6FE3FF'
  var LOGO = '<svg viewBox="0 0 512 512" width="128" height="128" aria-hidden="true" style="display:block;margin:0 auto">'
    + '<path fill="currentColor" d="M208 152C250 204 250 228 176 256C250 284 250 308 208 360L436 256Z"/></svg>'

  var KEYFRAMES = '@keyframes rv-in{from{opacity:0;transform:scale(.96)}to{opacity:1;transform:none}}'
    + '@keyframes rv-breathe{0%,100%{opacity:.35;transform:scale(.85)}50%{opacity:.8;transform:scale(1.08)}}'
    + '@keyframes rv-run{from{transform:translateX(-100%)}to{transform:translateX(250%)}}'
    + '#rivulet-boot-hint .rv-box{animation:rv-in .5s ease-out both}'
    + '#rivulet-boot-hint .rv-glow{animation:rv-breathe 2.4s ease-in-out infinite}'
    + '#rivulet-boot-hint .rv-run{animation:rv-run 1.4s ease-in-out infinite}'
    + '@media (prefers-reduced-motion:reduce){#rivulet-boot-hint .rv-glow,#rivulet-boot-hint .rv-run{animation:none}}'

  function starting() {
    if (hint || panel || !document.body)
      return

    var light = pale(GROUND_NOW)
    var ink = light ? '#1a1a1a' : '#e6e6e6'
    var quiet = light ? '#6a6a6a' : '#8a8a8a'
    // The brand cyan is only safe on the dark grounds it was drawn for; on a
    // light theme it falls under 3:1 at this size, so the mark goes to ink.
    var accent = light ? ink : BRAND

    hint = document.createElement('div')
    hint.id = 'rivulet-boot-hint'
    hint.setAttribute('style', 'position:fixed;top:0;left:0;right:0;bottom:0;z-index:2147483646;'
      + 'display:flex;align-items:center;justify-content:center;text-align:center;'
      + 'background:' + GROUND_NOW + ';color:' + ink + ';'
      + 'transition:opacity .3s ease;'
      + 'font:16px/1.5 Roboto,"Segoe UI",sans-serif;-webkit-text-size-adjust:none')

    var box = document.createElement('div')
    box.setAttribute('class', 'rv-box')
    box.setAttribute('style', 'position:relative')

    var mark = document.createElement('div')
    mark.setAttribute('style', 'position:relative;color:' + accent + ';margin:0 0 18px')
    mark.innerHTML = (light ? '' : '<div class="rv-glow" style="position:absolute;left:50%;top:64px;width:320px;height:320px;'
      + 'margin:-160px 0 0 -160px;border-radius:50%;pointer-events:none;'
      + 'background:radial-gradient(circle,rgba(111,227,255,.45) 0%,rgba(111,227,255,0) 65%)"></div>')
      + '<div style="position:relative">' + LOGO + '</div>'
      + '<div style="position:relative;margin-top:16px;font-size:40px;font-weight:700;letter-spacing:.14em;color:' + ink + '">Rivulet</div>'

    dots = document.createElement('div')
    dots.setAttribute('style', 'color:' + quiet + ';font-size:14px;letter-spacing:.06em;min-width:7em')
    dots.innerHTML = 'Starting'

    var track = document.createElement('div')
    track.setAttribute('style', 'position:relative;width:220px;height:3px;margin:18px auto 0;overflow:hidden;'
      + 'border-radius:3px;background:' + (light ? 'rgba(0,0,0,.1)' : 'rgba(255,255,255,.1)'))
    track.innerHTML = '<div class="rv-run" style="width:40%;height:100%;border-radius:3px;background:' + accent + '"></div>'

    var style = document.createElement('style')
    style.innerHTML = KEYFRAMES

    box.appendChild(mark)
    box.appendChild(dots)
    box.appendChild(track)
    hint.appendChild(box)
    hint.appendChild(style)
    document.body.appendChild(hint)

    pulse()
  }

  /**
   * The ellipsis, on a self-rescheduling timeout rather than an interval so that
   * dropping `dots` is the whole of stopping it — there is no handle to lose and
   * no clearInterval to reach a webview that might not have one.
   */
  function pulse() {
    if (!dots)
      return
    step = (step + 1) % 4
    dots.innerHTML = 'Starting' + Array(step + 1).join('.')
    setTimeout(pulse, 400)
  }

  /** Faded out rather than cut, so the app arrives instead of appearing. */
  function unhint() {
    var leaving = hint
    hint = null
    dots = null
    if (!leaving)
      return
    leaving.style.opacity = '0'
    setTimeout(function () {
      if (leaving.parentNode)
        leaving.parentNode.removeChild(leaving)
    }, 300)
  }

  /**
   * True once there is something worth interrupting the user for: the timeout
   * ran out, or an error arrived and the app did not recover from it.
   */
  var deep = false

  /**
   * Mounted wins, always. A weak box can take its time over 1.4 MB of
   * JavaScript, and an error early in boot doesn't always stop the app — so this
   * keeps looking rather than deciding once, and takes whatever is up back down
   * if the app turns up late.
   */
  function check() {
    if (mounted()) {
      unhint()
      hide()
    }
    else if (deep) {
      unhint()
      show()
    }
    else {
      starting()
    }
  }

  function escalate() {
    deep = true
    check()
  }

  setTimeout(check, HINT_MS)
  setTimeout(escalate, PANEL_MS)
  setInterval(function () {
    if (panel || hint)
      check()
  }, 1000)
})()
