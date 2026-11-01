#!/bin/sh
# Build the static site into OUTDIR (a folder GitHub Pages can serve):
#
#     crates/inspect-web/site.sh OUTDIR [BUNDLE_DIR]
#
# BUNDLE_DIR holds the public sheet bundles, `<drawing>/sh<N>/` (what
# `blobsketch bundle` writes; by default it is made here from 17619's sketch
# when the development repository's tools are present). The site is relative
# throughout (trunk's --public-url ./), so it works under any sub-path such as
# /apollo-lem-redrawings/. OUTDIR is emptied first.
#
# OUTDIR gets: index.html, the wasm and its script, theme.css, fonts (the subsets
# of fonts-src/, tools/publish/subset-fonts.sh), about.html,
# fallback.html (the page for a browser without WebGPU), index.json (the sheets),
# sheets/<drawing>/sh<N>/ (the bundles), .nojekyll and README.md.
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${1:?usage: site.sh OUTDIR [BUNDLE_DIR]}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT

BUNDLES="$2"
if [ -z "$BUNDLES" ]; then
  BS="$ROOT/tools/blobsketch/target/release/blobsketch"
  SKETCH="$ROOT/data/dev/LDW280-17619-sh3/LDW280-17619-sh3.sketch.json"
  [ -x "$BS" ] && [ -f "$SKETCH" ] || { echo "no BUNDLE_DIR given, and no blobsketch to make one" >&2; exit 1; }
  "$BS" bundle "$SKETCH" --out "$SCRATCH/bundles" >/dev/null
  BUNDLES="$SCRATCH/bundles"
fi
BUNDLES="$(cd "$BUNDLES" && pwd)"

# The built wasm carries the paths of the sources it was compiled from (panic
# messages); remap them so no home directory is in the published file.
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=~ --remap-path-prefix=$ROOT=."
(cd "$HERE" && trunk build --release --public-url ./ --dist "$SCRATCH/dist" >"$SCRATCH/trunk.log" 2>&1) \
  || { cat "$SCRATCH/trunk.log"; exit 1; }

find "$OUT" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
cp -R "$SCRATCH/dist/." "$OUT/"
# The loading indicator's length for the wasm when a server compresses it (the
# Content-Length is then the compressed length): the file's own size.
WASM="$(ls "$OUT"/*.wasm)"
sed -i.bak "s|var WASM_BYTES = 0;|var WASM_BYTES = $(wc -c < "$WASM" | tr -d ' ');|" "$OUT/index.html"
rm "$OUT/index.html.bak"
touch "$OUT/.nojekyll"

# The sheets, and their index.
python3 -I - "$BUNDLES" "$OUT" <<'PY'
import json, os, shutil, sys
src, out = sys.argv[1:3]
sheets = []
for drawing in sorted(os.listdir(src)):
    for sh in sorted(os.listdir(os.path.join(src, drawing))):
        d = os.path.join(src, drawing, sh)
        mpath = os.path.join(d, "manifest.json")
        if not os.path.isfile(mpath):
            continue
        m = json.load(open(mpath))
        dest = os.path.join(out, "sheets", drawing, sh)
        shutil.copytree(d, dest)
        fb = m.get("fallback", {})
        sheets.append({
            "id": f"{drawing}/{sh}",
            "title": f"{m['drawing']} sheet {int(m['sheet'])}" if str(m.get("sheet", "")).isdigit() else f"{drawing} {sh}",
            "path": f"sheets/{drawing}/{sh}/",
            "fallback": {k: v for k, v in fb.items() if k in ("redraw", "scan")},
        })
json.dump({"sheets": sheets}, open(os.path.join(out, "index.json"), "w"), indent=2)
open(os.path.join(out, "index.json"), "a").write("\n")
PY

COMMIT="${LMSHEETS_COMMIT:-}"
if [ -z "$COMMIT" ] && git -C "$ROOT" rev-parse --short HEAD >/dev/null 2>&1; then
  COMMIT="$(git -C "$ROOT" rev-parse --short HEAD)"
  [ -z "$(git -C "$ROOT" status --porcelain --untracked-files=no)" ] || COMMIT="$COMMIT (with uncommitted changes)"
fi
sed -e "s|@COMMIT@|${COMMIT:-unknown}|" -e "s|@DATE@|$(date -u +%Y-%m-%d)|" "$HERE/site-readme.md" > "$OUT/README.md"

echo "site: $OUT ($(du -sh "$OUT" | cut -f1), $(find "$OUT" -type f | wc -l | tr -d ' ') files)"

# Nothing private may be in it (the scan is the blobsketch tool's, when present).
BS="$ROOT/tools/blobsketch/target/release/blobsketch"
if [ -x "$BS" ]; then "$BS" privacy-scan "$OUT"; else echo "(privacy scan skipped: no blobsketch here)"; fi
