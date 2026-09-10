#!/bin/sh
# Copy public/ landing assets into cwd/.vercel-out.
# If Vercel already deleted public/ (output-dir wipe), restore it from git.
set -eu
ROOT=$(git rev-parse --show-toplevel)
if [ ! -f "$ROOT/public/landing.html" ]; then
  git -C "$ROOT" checkout HEAD -- public
fi
if [ ! -f "$ROOT/landing.html" ]; then
  git -C "$ROOT" checkout HEAD -- landing.html
fi
TMP=$(mktemp -d)
cp -a "$ROOT/public/." "$TMP/"
rm -rf "$TMP/.vercel-out" "$TMP/.vercel-static"
rm -f "$TMP/vercel.json"
cp -f "$ROOT/landing.html" "$TMP/landing.html"
rm -rf .vercel-out
mv "$TMP" .vercel-out
ls -la .vercel-out
test -f .vercel-out/landing.html
test -f .vercel-out/logo.svg
