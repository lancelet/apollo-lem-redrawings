#!/bin/sh
# Build the inspector and serve it with a bundle on http://127.0.0.1:8830/.
#
#     crates/inspect-web/serve.sh BUNDLE_DIR
#
# BUNDLE_DIR is what `blobsketch bundle --out DIR` wrote (DIR/<drawing>/sh<N>/),
# or a sheet directory itself (one with manifest.json). The app is built with
# trunk into this crate's dist/ (never `trunk serve`: it would not serve the
# bundle), then dist/ and the sheet are served side by side from a scratch
# directory of links by serve.py, in the background; a server already on the
# port is replaced. `--foreground` keeps it in this terminal instead.
set -e
PORT=8830
HERE="$(cd "$(dirname "$0")" && pwd)"
BUNDLE="${1:?usage: serve.sh BUNDLE_DIR [--foreground]}"
BUNDLE="$(cd "$BUNDLE" && pwd)"
if [ -f "$BUNDLE/manifest.json" ]; then
  SHEET="$BUNDLE"
else
  SHEET="$(dirname "$(find "$BUNDLE" -maxdepth 3 -name manifest.json | sort | head -1)")"
fi
[ -f "$SHEET/manifest.json" ] || { echo "no manifest.json under $BUNDLE" >&2; exit 1; }

(cd "$HERE" && trunk build --release)

ROOT="$HERE/.serve"
rm -rf "$ROOT"
mkdir -p "$ROOT"
for f in "$HERE"/dist/*; do ln -s "$f" "$ROOT/$(basename "$f")"; done
ln -s "$SHEET" "$ROOT/sheet"

if [ -f "$HERE/.serve.pid" ]; then kill "$(cat "$HERE/.serve.pid")" 2>/dev/null || true; fi
if [ "$2" = "--foreground" ]; then
  echo "http://127.0.0.1:$PORT/  (sheet: $SHEET)"
  exec python3 "$HERE/serve.py" "$ROOT" "$PORT"
fi
nohup python3 "$HERE/serve.py" "$ROOT" "$PORT" > "$HERE/.serve.log" 2>&1 &
echo $! > "$HERE/.serve.pid"
sleep 1
curl -s -o /dev/null "http://127.0.0.1:$PORT/" || { echo "server did not start; see $HERE/.serve.log" >&2; exit 1; }
echo "http://127.0.0.1:$PORT/  (sheet: $SHEET)"
