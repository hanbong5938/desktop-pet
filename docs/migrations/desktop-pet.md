# Legacy Herdr Desktop Pet → independent Desktop Pet

[Core installation](../../readme.md) · [Profile and discovery contract](../development.md#profiles-and-installation-discovery) · [Reviewed v0.4.0 candidate](../releases/v0.4.0.md) · [Herdr integration](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)

## Publication boundary

Legacy [v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6) is the shipped matched protocol-3 / observation-capability-1 app/helper. Its body, tag and four assets remain immutable. The bridge v0.3.7, independent v0.4.0, neutral formula and integration tags are not yet published. Actual candidate packaging/VM checks are separate evidence; final acceptance matrices and public cutover remain pending. Do not infer availability from a tag, default-branch code, a draft or version string. Use neutral public prebuilt/formula commands only after actual publication is verified; explicit `--source` builds the current core candidate and does not establish publication.

Desktop Pet's canonical owner is `hanbong5938/desktop-pet`; native app/helper are `DesktopPet.app`, `desktop-pet`, `desktop-pet-update-coordinator`, bundle ID `io.github.hanbong5938.desktop-pet`, protocol 4 / observation capability 2 / integration API 1. Herdr and Tern repositories own only their adapters/plugins. A redirect, Brew rename or permanent old-name CLI alias cannot migrate the old updater's image/origin proof.

## Preserve the exact profile

Back up **both actual config and state** before any operation, including `preferences.json`, `lifecycle.json`, `characters/`, `menu-bar-icons/` and updater evidence. Migration adopts the two existing directories in place; it does not move, merge, reset or delete their contents, locks or control socket locations. Isolated Tern, beta and custom profiles remain separate. A preexisting neutral default pointing elsewhere is not overwritten; continue with explicit paths. Deliberate `desktop-pet profile adopt --config-dir ABS --state-dir ABS` selects the pair only, subject to active-operation/live-daemon/ownership validation. See the [exact precedence and locator rules](../development.md#profiles-and-installation-discovery).

Migration keeps saved observation selection. Legacy `both` is a read-migration input normalized to `auto`; host-less legacy preferences retain Herdr, while new profiles default to Auto. Generic providers and capabilities, not source names or local-looking paths, decide authority.

Core owns common lifecycle persistence and `auto_start` (default on); registered integrations own their extra keys. The Herdr owner initializes `exit_with_herdr:true` **only when `lifecycle.json` is absent**, atomically in the shared profile writer on its first lifecycle write. An existing lifecycle file missing that key reads false; explicit false remains false. Legacy boolean `enabled`, including false, migrates to `auto_start`. Endpoints, startup metadata and unrelated owner fields survive; these settings live in `lifecycle.json`, not `preferences.json`. Bad JSON/values error rather than reset defaults, and failed saves preserve saved/runtime policy. Auto/no-adapter remain alive without Herdr; the following Herdr endpoint/disconnect rules apply only to Herdr-only selection.

## Operation A: shipped 0.3.6 → bridge 0.3.7

The old updater must first receive a real legacy envelope from the **old Herdr release authority**: `HerdrDesktopPet.app`, CLI `herdr-desktop-pet 0.3.7`, paired `herdr-update-coordinator`, bundle ID `dev.herdr.desktop-pet`, matched protocol 3 / observation capability 1. It is built from the same independent core revision with `DESKTOP_PET_DISTRIBUTION=legacy-0.3.7`, not a separate UI/code fork or a launcher that immediately execs the neutral image. The dedicated immutable v0.3.7 integration tag contains bridge metadata/downloader/wrapper, not copied core native/UI sources.

Check now alone changes nothing. Explicit Operation A consent authorizes only the exact bridge bytes. The old helper must observe the **actual bridge daemon's PID/image/version and same profile**, and complete A's terminal journal, lease and reservation release before B is enabled. A retains the old updater's rollback limitations; a successful A is not neutral migration.

## Operation B: explicit bridge → neutral 0.4.0

Only the separate **Migrate to Desktop Pet** consent authorizes B. It shows the authenticated source/target installation, both preserved profile paths, manager changes and exact legacy retirement. A verified already-installed neutral image may be reused rather than installing a second app.

A staged, verified **protocol-4** helper executes `migrate-legacy --plan ABS`; it is not disguised as the bridge's paired protocol-3 helper. The version-1 `legacy_to_desktop_pet` plan records operation ID, authenticated source image/origin/profile/instance/stop-generation, neutral target release/image/origin and prior locator/default-profile record. Bridge-only bounded `legacy-migration` Prepare/Stop authorization does not change normal old protocol 3; neutral readiness uses normal protocol 4. Unresolved old operations, stale authorization, another live root-sharing profile or changed image/hash block before mutation.

Stage and validate the neutral app **before** stopping the bridge. Quiesce every profile using the source root, stop the authorized bridge, atomically switch the locator and launch the exact neutral image with the same config/state. Verify actual PID/path/hash, `app_version:0.4.0`, protocol 4 / capability 2, preserved observation selection, same profile and real UI/control readiness. Before migration commit, target mutation remains gated: no new preference/schema/agent writes.

On readiness failure restore the exact bridge image and prior locator/default-profile record. Restart it only if no user Stop occurred and old authorization is still valid. **User Stop forbids target restart and rollback start.** If manager execution is Unknown, never rerun it automatically: preserve both images, signed evidence and journals for explicit reconciliation. Installed is not applied; native ready is not migration complete.

Only after successful neutral readiness/commit may the owning manager retire the authorized old installation:

- Brew: uninstall only the approved old stable formula.
- Herdr managed: the same registry refreshes to the integration-only v0.4.0 immutable ref, then removes its generated plugin-local app. This is a **one-time migration action**, never the normal native updater.
- Linked/local source: preserve checkout, user changes and Git origin; remove only the verified generated legacy bundle, never reset/delete source.
- Manual archive: remove only the approved exact bundle.

Failed retirement remains a distinct incomplete state: show the old root inactive and allow only the journal's explicit resume. Retain rollback evidence/old journals, but do not expose a live legacy app or permanent old-name alias as ordinary discovery. The app does not edit Tern KV: Configure the verified neutral executable with the **same existing config/state pair**, save successfully, Connect, and verify the actual Tern window/native connection before declaring that integration migrated. Cancel/failure preserves the previous triple and uncertain/offered prompts are not replayed.

## Protocol-2 beta and older clients

The retained `0.3.5-beta.1` app/helper is protocol 2 and cannot directly receive bridge protocol 3 in-app. First settle its old operation using the retained **matching signed helper**, then stop the actual beta daemon. Never reserialize old protocol-1/2/3 evidence to v4 or recover it with the new helper.

Use the **frozen, pinned v0.3.6 prebuilt** to obtain the matched protocol-3 pair. The dedicated bridge ingress provides `bash scripts/install-0.3.6.sh --legacy-stopped`; its pin is the immutable legacy archive, not the evolving stable formula. The ingress implementation is owned by core `migration/legacy-0.3.7/ingress`, then materialized as the original Herdr repository’s dedicated immutable `v0.3.7` tag root. After that tag is actually published, clone the Herdr integration repository and check out the immutable `v0.3.7` ingress (not current integration-only main). Run the pinned installer from that tag root, with its companion install/validation scripts present, then start its `dist/HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet` with the preserved explicit config/state pair. Verify actual running image/version `0.3.6` and matched protocol 3, then perform A → B. Never describe this manager transition as automatic beta in-app migration or claim the current stable formula still supplies 0.3.6 once it rolls to bridge.

The exact archive is [HerdrDesktopPet-v0.3.6-macos-arm64.tar.gz](https://github.com/hanbong5938/herdr-desktop-pet/releases/download/v0.3.6/HerdrDesktopPet-v0.3.6-macos-arm64.tar.gz), SHA-256 `5d40f6ada23f7d91d967a98331b28ea767202f5d01b7cd7732b2d9e543fdaafa`; use its matching checksum/provenance assets and verified installer, not an unpinned latest download. Historical beta formula/assets remain unchanged; no new beta publication is implied.

## Explicit source/link/manual alternative

After backing up the actual profile, settling pending old operations and stopping the old daemon, source/link/manual users may explicitly install from the core prebuilt installer/new formula or build the core with `--source`, then adopt the same pair. Do not rename the old repository, change its origin or pretend it is the core. Native plugin linkage still belongs to the integration checkout. Default installation is prebuilt-only, never a silent source fallback.

## Recovery and real acceptance

Protocol-4 journals use `updates-v4/` and actual OS account-home `.desktop-pet-updates`; old `updates/` and `.herdr-updates` evidence remains authoritative. Lock order across relevant start/stop/update/migration operations is old global gate → new global gate → sorted installation roots, including legacy stop-generation checks. Unknown manager execution, active legacy reservations and stale authorization fail closed.

Run actual installs, upgrades, retirement and rollback only in a **disposable macOS account or VM with a logged-in Window Server**. Changing HOME/XDG is not sufficient isolation: global gates use the real OS account home. Never delete the user's real plugin, agents or worktrees for a smoke test. Record both images/PIDs/profiles and character-registry digests, separate installed/applied/retired outcomes and actual Herdr/Tern reconnect evidence. Fixture tests, prior legacy smoke or source metadata cannot replace these runtime and publication gates. They have not been exercised by this documentation change.
