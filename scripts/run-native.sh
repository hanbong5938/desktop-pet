#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
source "$ROOT_DIR/scripts/profile-paths.sh"
binary="$ROOT_DIR/dist/DesktopPet.app/Contents/MacOS/desktop-pet"
helper="$ROOT_DIR/dist/DesktopPet.app/Contents/MacOS/desktop-pet-update-coordinator"
fail() { printf 'desktop pet development: %s\n' "$*" >&2; exit 1; }
[[ "${DESKTOP_PET_DISTRIBUTION:-neutral}" == neutral ]] || fail 'use the bounded legacy envelope launcher for a bridge build'
[[ -f "$binary" && ! -L "$binary" && -x "$binary" && -f "$helper" && ! -L "$helper" && -x "$helper" ]] || fail "core app is not packaged; run bash scripts/install.sh --source first: $binary"
/usr/bin/codesign --verify --deep --strict "$ROOT_DIR/dist/DesktopPet.app" || fail 'core development bundle has no valid signature'

# Development uses its actual absolute core image, never a plugin-local app or
# PATH fallback, and must not silently attach to the shared/default profile.
config="" state="" next=""
for arg in "$@"; do
  if [[ -n "$next" ]]; then
    case "$next" in config) config="$arg" ;; state) state="$arg" ;; esac
    next=""
  else
    case "$arg" in --config-dir) next=config ;; --state-dir) next=state ;; esac
  fi
done
[[ -z "$next" && "$config" == /* && "$state" == /* ]] || fail 'pass both --config-dir ABS and --state-dir ABS for a separate development profile'
default_profile="$(env -u DESKTOP_PET_CONFIG_DIR -u DESKTOP_PET_STATE_DIR "$binary" profile show --json)" || fail 'cannot inspect the default profile'
default_config="$(/usr/bin/plutil -extract config_dir raw -expect string -o - - <<< "$default_profile")"
default_state="$(/usr/bin/plutil -extract state_dir raw -expect string -o - - <<< "$default_profile")"
require_separate_development_profile "$config" "$state" "$default_config" "$default_state" || exit 1
"$helper" discover --executable "$binary" --json >/dev/null || fail 'core development image failed native installation/origin validation'
exec "$binary" "$@"
