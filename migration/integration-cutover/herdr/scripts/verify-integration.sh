#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
[[ "$#" -le 1 ]] || { echo 'Usage: bash scripts/verify-integration.sh [RETAINED_HERDR_SOURCE]' >&2; exit 1; }
SOURCE_DIR="$(cd -- "${1:-$ROOT_DIR}" && pwd -P)"
for path in native-adapter/Cargo.toml native-adapter/Cargo.lock native-adapter/src/lib.rs integrations/herdr/client-attached.patch integrations/omp/herdr-pet-outcomes.ts; do
  [[ -f "$SOURCE_DIR/$path" ]] || { echo "Missing retained integration source: $SOURCE_DIR/$path (pass the Herdr checkout when verifying the unmaterialized candidate)" >&2; exit 1; }
done
# Inspect the actual candidate/final root, not just the disposable overlay.
# Borrowed source may still contain the legacy app until the parent's cutover.
for obsolete in native web assets site scripts/package-native.ts scripts/build-rig-native.mjs scripts/release-docs.ts scripts/release-docs.test.ts scripts/wiki-docs.ts scripts/wiki-docs.test.ts scripts/publish-wiki.sh tools/pages.mjs tools/pages.test.ts tools/adapt-rig-source.py tools/character-pack.py tools/repair-hair-pockets.mjs tools/repair-hair-pockets.test.ts tools/native-rig-probe tools/rig-probe .agents/skills/character-creator .agents/skills/create-pet-character .github/workflows/wiki.yml .github/workflows/pages.yml; do
  [[ ! -e "$ROOT_DIR/$obsolete" && ! -L "$ROOT_DIR/$obsolete" ]] || { echo "Obsolete common product owner remains: $obsolete" >&2; exit 1; }
done
# Overlay only the retained closure into disposable storage. This verifies the
# candidate before core publication without modifying the original source tree.
stage="$(mktemp -d "${TMPDIR:-/tmp}/herdr-integration-verify.XXXXXX")"
trap 'rm -rf -- "$stage"' EXIT
mkdir -p "$stage/scripts" "$stage/native-adapter"
cp "$ROOT_DIR/package.json" "$ROOT_DIR/bun.lock" "$ROOT_DIR/tsconfig.json" "$ROOT_DIR/herdr-plugin.toml" "$stage/"
cp "$ROOT_DIR/scripts/"*.sh "$ROOT_DIR/scripts/integration-release.ts" "$stage/scripts/"
cp "$SOURCE_DIR/native-adapter/Cargo.toml" "$SOURCE_DIR/native-adapter/Cargo.lock" "$stage/native-adapter/"
cp -R "$SOURCE_DIR/native-adapter/src" "$stage/native-adapter/src"
cp -R "$SOURCE_DIR/integrations" "$stage/integrations"
for script in "$stage/scripts/"*.sh; do bash -n "$script"; done
cd -- "$stage"
bun scripts/integration-release.ts source
bun install --frozen-lockfile
bun run check
cargo test --locked --manifest-path native-adapter/Cargo.toml
printf 'Integration-only shell, manifest, pinned adapter/contracts and OMP TypeScript verification completed. No app was built or release published.\n'
