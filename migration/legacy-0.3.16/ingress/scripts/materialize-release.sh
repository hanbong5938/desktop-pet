#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
source "$ROOT_DIR/scripts/validate.sh"
[[ "$#" == 4 ]] || fail 'Usage: bash scripts/materialize-release.sh ARCHIVE RIG_PROVENANCE RELEASE_RECORD NEW_OUTPUT_DIRECTORY'
archive="$1"
provenance="$2"
record="$3"
output="$4"
[[ "$output" == /* && ! -e "$output" && ! -L "$output" ]] || fail 'output must be a new absolute directory; existing trees/tags are never overwritten'
for input in "$archive" "$provenance" "$record"; do
  [[ -f "$input" && ! -L "$input" ]] || fail "missing real packaging input: $input"
done
[[ "${archive##*/}" == HerdrDesktopPet-v0.3.16-macos-arm64.tar.gz ]] || fail 'wrong bridge archive name'
require_value "$record" version 1
require_value "$record" distribution legacy-0.3.16
require_value "$record" app_version 0.3.16
require_value "$record" archive HerdrDesktopPet-v0.3.16-macos-arm64.tar.gz
require_value "$record" provenance HerdrDesktopPet-v0.3.16-rig-native.json
require_value "$record" protocol 3
require_value "$record" observation_mode 1
require_value "$record" integration_api 1
revision="$(json_value "$record" source_core_revision)"
[[ "$revision" =~ ^[0-9a-f]{40}$ ]] || fail 'actual packaged source_core_revision is required (exact lowercase 40-hex); no HEAD inference or placeholder'
migration_revision="$(json_value "$record" herdr_migration_revision)"
[[ "$migration_revision" == "$retirement_revision" ]] || fail 'frozen Herdr v0.4.1 retirement authority required'
require_value "$provenance" distribution_identity.herdr_migration_revision "$migration_revision"
require_value "$provenance" version 1
require_value "$provenance" core_revision "$revision"
require_value "$provenance" distribution_identity.core_revision "$revision"
require_value "$provenance" distribution_identity.distribution legacy-0.3.16
require_value "$provenance" distribution_identity.app_version 0.3.16
require_value "$provenance" distribution_identity.app_name HerdrDesktopPet.app
require_value "$provenance" distribution_identity.executable herdr-desktop-pet
require_value "$provenance" distribution_identity.helper herdr-update-coordinator
require_value "$provenance" distribution_identity.bundle_id dev.herdr.desktop-pet
require_value "$provenance" distribution_identity.protocol 3
require_value "$provenance" distribution_identity.observation_mode 1
require_value "$provenance" distribution_identity.integration_api 1
digest="$(sha256_file "$archive")"
require_value "$record" archive_sha256 "$digest"
provenance_digest="$(sha256_file "$provenance")"
# Construct off to the side; a failed copy/write leaves no apparently complete tag tree.
parent="$(cd -- "$(dirname -- "$output")" && pwd -P)" || fail 'output parent must already exist'
leaf="$(basename -- "$output")"
[[ "$leaf" != . && "$leaf" != .. && "$leaf" != / ]] || fail 'output must name a new directory'
output="$parent/$leaf"
[[ ! -e "$output" && ! -L "$output" ]] || fail 'physical output already exists; never overwrite an ingress tree'
workspace="$(/usr/bin/mktemp -d "$parent/.bridge-ingress-materialize.XXXXXX")" || fail 'cannot reserve physical materialization staging directory'
archive_scratch=""
trap '[[ -z "$archive_scratch" ]] || rm -rf "$archive_scratch"; rm -rf "$workspace"' EXIT
# Admit actual archive bytes before creating any release tree. Validation uses
# private disposable profiles and never the caller's config/state or daemon.
archive_scratch="$(/usr/bin/mktemp -d /private/tmp/legacy-ingress-materialize-validation.XXXXXX)" || fail 'cannot reserve private archive validation directory'
extract_archive "$archive" "$archive_scratch"
validate_app "$archive_scratch/extract/HerdrDesktopPet.app" 0.3.16
rm -rf "$archive_scratch"
archive_scratch=""
stage="$workspace/$leaf"
mkdir "$stage"
mkdir "$stage/scripts"
cp "$ROOT_DIR/herdr-plugin.toml" "$stage/herdr-plugin.toml"
cp "$ROOT_DIR/.gitignore" "$stage/.gitignore"
for script in install.sh run-native.sh validate.sh install-0.3.6.sh; do
  cp "$ROOT_DIR/scripts/$script" "$stage/scripts/$script"
  chmod 755 "$stage/scripts/$script"
done
printf '{\n  "version": 1,\n  "repository": "hanbong5938/herdr-desktop-pet",\n  "source_core_repository": "hanbong5938/desktop-pet",\n  "source_core_revision": "%s",\n  "distribution": "legacy-0.3.16",\n  "app_version": "0.3.16",\n  "protocol": 3,\n  "observation_mode": 1,\n  "integration_api": 1,\n  "archive_url": "https://github.com/hanbong5938/herdr-desktop-pet/releases/download/v0.3.16/HerdrDesktopPet-v0.3.16-macos-arm64.tar.gz",\n  "checksum_url": "https://github.com/hanbong5938/herdr-desktop-pet/releases/download/v0.3.16/HerdrDesktopPet-v0.3.16-macos-arm64.tar.gz.sha256",\n  "archive_sha256": "%s",\n  "provenance_sha256": "%s"\n}\n' "$revision" "$digest" "$provenance_digest" > "$stage/release.json"
plutil -insert herdr_migration_revision -string "$migration_revision" "$stage/release.json" || fail 'cannot preserve the authenticated Herdr retirement revision'
plutil -convert json "$stage/release.json" || fail 'cannot encode materialized release metadata as JSON'
cp "$record" "$stage/packaging-release.json"
cp "$provenance" "$stage/rig-native-provenance.json"
[[ ! -e "$output" && ! -L "$output" ]] || fail 'output appeared while staging; never replace an existing tree'
# Moving the named child into its physical parent avoids directory-target
# nesting, and -n also refuses a destination created after the preceding check.
mv -n "$stage" "$parent/" || fail 'cannot publish the new materialized directory'
[[ ! -e "$stage" ]] || fail 'output appeared during publication; existing tree preserved'
printf 'Materialized metadata-only bridge ingress at %s from actual core %s; not published.\n' "$output" "$revision"
