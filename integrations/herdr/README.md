# Desktop Pet — Herdr integration

This repository owns the Herdr adapter, plugin manifest, thin consumer wrappers, host patch and OMP outcomes extension—not the native app/UI/characters/build. [Desktop Pet core installation and usage](https://github.com/hanbong5938/desktop-pet) · [exact profiles/discovery](https://github.com/hanbong5938/desktop-pet/blob/main/docs/development.md#profiles-and-installation-discovery) · [legacy migration entrance](https://github.com/hanbong5938/desktop-pet/blob/main/docs/migrations/desktop-pet.md).

**Publication boundary:** legacy [Herdr v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6) is shipped with protocol 3 / observation capability 1. Independent Desktop Pet v0.4.0 (protocol 4 / observation capability 2 / integration API 1) and the legacy bridge v0.3.7 are reviewed candidates, not verified public installations. Commands below describe the neutral cutover; do not treat notes, a source version or a tag as publication proof.

**Deferred integration cutover:** the integration-only manifest/wrappers are reviewed candidates in the core checkout's `migration/integration-cutover/herdr`, not yet the installed/default-branch consumer. Materialization requires the actual anonymously verified core installer/checksum and reviewed release API metadata; only then may the parent apply the integration cutover. The legacy originals remain preserved until that step. Source/link and consumer instructions below describe the resulting consumer, not evidence that this checkout already provides it.

## Consumer installation

After actual neutral release/formula publication:

```sh
brew install hanbong5938/tap/desktop-pet
herdr plugin install hanbong5938/herdr-desktop-pet
herdr plugin action invoke start --plugin desktop-pet
```

The integration installer reuses a verified compatible installation with `integration_api:1`; only absent native installation permits fetching the pinned, digest-verified core release installer into the shared standalone location. It does not infer core version from this plugin version, downgrade/replace an incompatible installation, compile core here or hide download failure behind source fallback. Native source/prebuilt installation belongs exclusively to the core checkout (`--source`, not `--local-build`). The wrapper’s discovery order is explicit absolute path → validated shared locator → PATH `desktop-pet` → actual Brew prefix → account-home `Applications/DesktopPet.app`; invalid explicit path/locator fails closed. A locator alone is not capability/signature/origin proof, and a different running image/profile is shown rather than silently launched.

Herdr `HERDR_PLUGIN_CONFIG_DIR` / `HERDR_PLUGIN_STATE_DIR` precedence is interpreted **only by this wrapper**, then passed as explicit native paths. Core defaults instead use neutral environment/default-profile record/XDG roots. Select and back up both actual directories; never move/merge legacy, isolated Tern, beta or custom profiles. Backend socket provenance becomes adapter-owned `--herdr-socket ABS`; it is not the pet’s private profile control socket. `status` reports the actual daemon executable/profile/version, not just the installed CLI. Manual local snapshots (Herdr 0.9.0+) need no plugin/patch; replies separately need server `agent.prompt`, and its ACK is not model completion.

`herdr plugin install` reruns integration update only. `plugin uninstall/unlink` does not delete the shared native app/profile/characters. Native updates never reinstall this plugin, except the explicitly consented **one-time managed legacy migration retirement** using the same registry and integration-only immutable v0.4.0 ref. Auto retains the pet/other provider when Herdr disappears. Herdr-only disconnect/disable lifecycle remains adapter-owned. Tern's separate plugin neither installs nor starts the native app.

## Optional client-attach auto-start

Stock Herdr 0.9.0/0.9.3 do not provide `client.attached`. The manifest minimum is not hook proof; optional attach auto-start requires official Herdr 0.9.3 built with this repository’s [client-attached.patch](client-attached.patch), or an actual host release advertising that hook. Plugin installation never patches Herdr. Linking/enabling a running host does not itself start the pet: invoke start once or await a later eligible shell/terminal attach. `ensure` is idempotent, respects saved provider/profile and never reveals a hidden pet. Inspect/reject unknown hook warnings rather than treating a successful link as host compatibility.

## Build in separate checkouts

Prerequisites: macOS arm64 for the core; Rust/Cargo, Bun, npm, Xcode command-line build tools and the tools needed by the independent core and host source trees. Build the Herdr host with the repository's normal locked Rust toolchain and native dependencies (including Zig 0.16.0 for vendored libghostty-vt); consult the official Herdr `README.md` and `AGENTS.md`. `just` is needed for its test/check recipes. For the full `just check` on macOS, the Herdr Windows cross target additionally needs `cargo install xwin --locked` and `just setup-windows-cross` (interactive SDK license acceptance; see Herdr `CONTRIBUTING.md`). Ordinary macOS builds do not need the Windows SDK.

From the **Herdr integration** checkout, keeping its absolute path for linking (the independent core is a separate checkout):

```bash
INTEGRATION_ROOT="$PWD"
HERDR_SOURCE="$(mktemp -d "${TMPDIR:-/tmp}/herdr-v0.9.3-patched.XXXXXX")"
git clone https://github.com/herdrdev/herdr.git "$HERDR_SOURCE"
cd "$HERDR_SOURCE"
git checkout 7b116c05bfda646af39d2524c54e70c751f57ee8
git rev-parse HEAD  # must be 7b116c05bfda646af39d2524c54e70c751f57ee8
git apply --check "$INTEGRATION_ROOT/integrations/herdr/client-attached.patch"
git apply "$INTEGRATION_ROOT/integrations/herdr/client-attached.patch"
just test
just check       # after setting up the Windows cross target as described above
cargo build --release --locked
```

Use `"$HERDR_SOURCE/target/release/herdr"` explicitly as `PATCHED_HERDR` below. Do **not** copy it over an installed Herdr, replace an existing server, or change your update channel implicitly. Start and inspect a separate disposable Herdr session with the patched binary (and avoid inherited `HERDR_SOCKET_PATH` / `HERDR_CLIENT_SOCKET_PATH` from another session; see the host's `AGENTS.md`). API/schema checks must use the *same patched binary* and target session, not a stable binary already on `PATH`.

Install or explicitly build the core first in its own checkout with `bash scripts/install.sh --source`. Then return to this integration checkout and link it with the patched host. Herdr `plugin link` does not execute a build command:

```bash
cd "$INTEGRATION_ROOT"
PATCHED_HERDR="$HERDR_SOURCE/target/release/herdr"
HOST_PROFILE="$(mktemp -d /tmp/herdr-pet.XXXXXX)"
export XDG_CONFIG_HOME="$HOST_PROFILE/config" XDG_STATE_HOME="$HOST_PROFILE/state"
export XDG_DATA_HOME="$HOST_PROFILE/data" XDG_CACHE_HOME="$HOST_PROFILE/cache"
unset HERDR_ENV HERDR_SOCKET_PATH HERDR_CLIENT_SOCKET_PATH HERDR_CONFIG_PATH
unset HERDR_PLUGIN_CONFIG_DIR HERDR_PLUGIN_STATE_DIR
"$PATCHED_HERDR" server &
# Wait for this server's "api socket:" message before the commands below.
"$PATCHED_HERDR" plugin link "$INTEGRATION_ROOT"
"$PATCHED_HERDR" plugin list
"$PATCHED_HERDR" api schema --json
```

Inspect the `plugin list`/`plugin.link` warnings for `unknown event 'client.attached'`; no such warning should appear. Inspect the printed API schema for a `client.attached` subscription and `client_attached` event. An explicit raw API probe on the patched session is `{"id":"attach_probe","method":"events.subscribe","params":{"subscriptions":[{"type":"client.attached"}]}}` sent to its API socket: require a successful subscription acknowledgment, attach a **shell or terminal** client to the same session, and observe an event with `event: "client_attached"`, `data.type: "client_attached"`, server-local numeric `data.client_id`, and `data.client_kind: "shell"` or `"terminal"`. A plain API request, failed client handshake, or handoff must not emit attach. A successful link or version match alone is **not** this capability check.

## Adapter-owned lifecycle

`auto_start` controls Herdr server/client hooks, never Tern autostart. `exit_with_herdr` and endpoint data remain Herdr-owned keys in `lifecycle.json`; common and adapter keys persist atomically without dropping each other. In Herdr-only mode, enabled exit policy waits 30 seconds after loss of the last healthy server; turning it off retains independent operation. Fresh Herdr policy defaults on; a missing exit key in an existing profile retains the historical off migration. Client detach is not server death. Initially absent plugin membership is standalone observation, not disable; confirmed disable/unlink of every registered local endpoint triggers the selected Herdr-only policy. Auto/other provider/zero-host runs do not acquire Herdr startup/host-loss requirements. Keep `auto_start` off if later eligible hooks must not relaunch after Stop.

Remote Herdr observation is read-only, configured via selected saved-machine profiles and interactive authentication in the actual Herdr terminal. It needs a compatible local forwarding CLI/remote server, not a remote pet install. Local-looking paths never upgrade remote mutation authority. Use core [usage](https://github.com/hanbong5938/desktop-pet/blob/main/docs/usage.md) and [CLI](https://github.com/hanbong5938/desktop-pet/blob/main/docs/cli.md) for generic controls and status.

## OMP outcome setup

The extension remains owned here at [`../omp/herdr-pet-outcomes.ts`](../omp/herdr-pet-outcomes.ts). Load it **alongside**, never instead of or by modifying, Herdr’s managed OMP state extension:

```sh
HERDR_INTEGRATION=/absolute/path/to/current/herdr-desktop-pet-checkout
omp --extension "$HERDR_INTEGRATION/integrations/omp/herdr-pet-outcomes.ts"
```

Use OMP with the `agent_end.willContinue` contract (the historical producer/consumer evidence used OMP 18.2.10). Preserve `omp:v1` / `pet_outcome_*` wire authority; acceptance/idle is not synthetic completion. Character-authoring packs/catalog and rig ABI remain core/catalog concerns, not host protocol revisions.

## Migration and acceptance

Shipped legacy 0.3.6 must perform **A → real legacy bridge 0.3.7**, wait for A terminal/lease/reservation release, then separately consent to **B → neutral 0.4.0**. Same-profile image/readiness proof, user Stop, Unknown/no manager replay, exact rollback and owner-only legacy retirement are required; runtime-ready without retired old install is not complete. Protocol-2 beta uses its retained matching helper, actual stop and **pinned frozen 0.3.6** prebuilt before A→B, not the current rolling stable formula. Tern Configure keeps its old two paths and requires actual reconnect; the app never edits its KV. See the [full migration contract](https://github.com/hanbong5938/desktop-pet/blob/main/docs/migrations/desktop-pet.md).

All actual installation/migration/recovery smoke must run in a **disposable macOS account or VM**, not merely changed HOME/XDG: global gates use the real account home. The user's real plugin, agents and worktrees are not disposable fixtures. No neutral/bridge runtime or public cutover success is asserted here; historical releases remain unchanged.
