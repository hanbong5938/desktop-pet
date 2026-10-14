# Desktop Pet

An independent native macOS desktop companion. Rubelia reacts to touch, displays sessions from registered providers such as Herdr and Tern, and lets you reply when that source grants prompt capability. The app also runs without a host.

**English** · [한국어](readme.ko.md) · [简体中文](readme.zh-Hans.md) · [日本語](readme.ja.md) · [Release history](docs/releases/README.md) · [Core releases](https://github.com/hanbong5938/desktop-pet/releases)

<img src="assets/rubelia-thumbnail.png" alt="Rubelia, the default desktop companion" width="220">

[Browse the optional character and outfit gallery](https://hanbong5938.github.io/herdr-characters/) (opens in English; choose KO for Korean) · [Character pack source and licenses](https://github.com/hanbong5938/herdr-characters). **Only Rubelia (`default@0`) is bundled.**

**Current public Core:** [Desktop Pet v0.4.4](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.4), release `412446874`, published `2026-10-14T07:31:12Z`, immutable source `b7ab7261aa239441273065e1df6ba7402c750454`. All six anonymous public assets, strict/deep signatures, paired protocol 4 / capability 2 / API 1, all 68 deployed hashes and exact regular-file closure passed. Archive SHA-256 `be59d299cdfc631858239e729186c09951da57811635fea3adeab9e21941695c`; installer SHA-256 `4a2b5d222513cae77045229e62f59e2b4d0c871cab0f1904ee1108be00acef33`. [Release CI 38528323235](https://github.com/hanbong5938/desktop-pet/actions/runs/38528323235), release job `115733881402`, passed. The later Wiki job `115735603636` actually rendered **38 managed pages on Linux**, then failed with `WIKI_PUBLISH_TOKEN is missing; use a dedicated credential with Wiki write access`. Wiki publication still requires owner initialization of Wiki Home and a dedicated Wiki-write secret; never reuse a broad owner token or rerun the binary Release for Wiki repair. Final current Pages deployment and all four locale renders remain unverified.

The neutral v0.4.4 formula is public at tap `2aa9a4be9517e0232a3f2b8e2b3c748045c69586`. A qualified fresh Brew v0.4.4 install passed (74 files / 44.3 MB and five inner/outer re-signatures); authenticated inspection verified the Homebrew-derived image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312`. Install/inspection created no neutral config/state or standalone app and did not start a daemon. Only explicit manual **Start** produced actual v0.4.4 UI/control readiness, PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470`. Rubelia and actual Settings showed **Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0**; explicit Hidden afterward preserved that default instance. This is fresh-install proof, not a v0.4.4 normal-update or managed-migration result.

**Current public bridge:** [Herdr bridge v0.3.9](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.9), release `412458870`, published `2026-10-14T07:47:14Z`, source `0f33f2806412fe814367f58bdfbbe2e833399a3e`, tree `1572984289c3eb2049b4d5b17c27b013c2ca593a`. Its exact **11 regular UTF-8 Git files** contain metadata/downloader/license/readme and only the narrow `/dist/HerdrDesktopPet.app/` ignore. All six producer scripts/manifest/ignore blobs match public Core `b7ab7261aa239441273065e1df6ba7402c750454`; retirement stays at Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`. Five anonymous public assets match independently verified CI bytes, strict signatures, matched protocol 3 / capability 1 / API 1 and exact 68-file hash closure. Archive SHA-256 `061dc6cb04e1512db90f0ffe1e9e261fd2be0935924e189ca0c7f8ce3c43808c`. [Bridge CI 38529018457](https://github.com/hanbong5938/desktop-pet/actions/runs/38529018457), job `115736118653`, passed with actual shipped-native `TMPDIR=/tmp` and real Git admission: the generated app stays clean; tracked edits, unrelated root files and `dist/user-owned-note` stay dirty. Old stable formula tap `349e3074ef41960096fbfad884f0ef4b17930e0f` selects v0.3.9. Frozen v0.3.6 remains historical latest; beta formula/tags/assets are unchanged. 

**Actual final public managed A completed:** Frozen v0.3.6 Check selected public v0.3.9 and changed only `preferences/last_update_check` (`1791963743` → `1791965310`); all other four-tree bytes, source PID `2467` and unrelated default PID `2641` stayed unchanged during Check. Exact-source `0f33f28` native A consent completed operation `d5b2bf6f02e031eabb5a8c8fdd805cab` (`completed` / `manager_exited`, installed/applied true). Actual public v0.3.9 native PID `3417` / instance `b42b25d48315e66bd87e7685e58cfb14`, image `0ed12d159b6bbb83ada833d39b4524ac5f7eccb1253b7acaf9aa2b71cbf833ce`, reached matched protocol 3 / capability 1 / API 1, UI/control ready, Herdr 1 / Tern 0 on the exact private old config/state. The actual managed source Git status was fully clean. The original retained v0.3.6 helper passed strict/deep signatures and its actual status confirmed completed. Baselines hash all 87 old A-evidence files and 91 total state files. 

**Actual final public managed B completed:** A separate Move preview and independent strict/deep-verified public v0.4.4 coordinator received native **Migrate** consent for operation `d729fd740f4e459945edda00bf48d944`, plan SHA-256 `6471e928254229a66a6fc64a88f67095777d6bd890d621feb36b08a4c7d1fc69`, and reached **COMPLETE**. Native Prepare/Stop of public v0.3.9 PID `3417` / instance `b42b25d48315e66bd87e7685e58cfb14` selected the already-qualified existing Brew v0.4.4 app root/image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312`; `target_manager:null` means **no reinstall**. Target PID `4104` / instance `8912b3aad5419b1c14a78ef3418ae36f` reached protocol 4 / capability 2 / API 1 and actual UI/control ready on the exact old private config/state, preserving saved Herdr selection (Herdr 1 / Tern 0); native_ready and target_committed were true. Genuine Herdr retirement attempt 1 used PID = PGID `4140`, exit 0, quiescent true, error null. Actual plugin list showed enabled integration-only Desktop Pet at immutable Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`; managed Git HEAD matched, and real manager output reported v0.4.1, 21 actions, startup/client-attached hooks. Managed dist/app and old PID `3417` were gone; no standalone `DesktopPet.app` was created. The exact retired v0.4.1 tag’s historical Core v0.4.2-minimum wrapper actually reused the same v0.4.4 PID/instance—no new app or tag. All 87 original A-evidence SHA-256 values and all preexisting old source-config hashes were unchanged; both unrelated default config/state trees were byte-identical, with hidden default v0.4.4 PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470` unchanged and running. The private XDG base profile was adopted, not merged into the global default. Actual GUI was visually read as Desktop Pet / built-in Rubelia and Settings **Herdr Connected 1 / Tern Disabled 0**; clean-image capture remains pending. This fault-free final public v0.3.9/v0.4.4 A/B happy path is distinct from the historical working-metadata `f7edf5e` definite-failure/explicit-Resume qualification, which separately passed. **Actual beta ingress, a neutral v0.4.4 normal update, real-SDK both-order/prompt/transcript/reconnect acceptance and final current Pages remain pending; the full matrix is not complete.**

**Consumer and SDK boundaries:** Herdr integration stays public v0.4.1 at `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8` (assetless release `412252863`); its historical Core v0.4.2 minimum stays frozen and healthy Core v0.4.3 reuse was observed. Current Herdr consumer scripts and all eight metadata/evidence files are now actually materialized from public Core v0.4.4 release `412446874` / source `b7ab7261aa239441273065e1df6ba7402c750454`, not a new Herdr tag; final public managed retirement and the immutable wrapper’s v0.4.4 reuse are separately verified above. Tern v0.2.0 and its compatible Core v0.4.1 pin remain unchanged. Tern requires explicit **Configure → Save → Connect** of the verified executable and actual config/state pair; it never installs or auto-starts the app. Real Tern keychain/provider sign-in and genuine OMP 18.10.3 providerSignIn/NoModel still block both-order same-instance/prompt/transcript/reconnect acceptance. No credential copy, fake account, ACK or fabricated completion substitutes for those gates. **The full matrix is not complete.**

**Historical v0.4.2 / v0.3.7 evidence (not v0.4.4 runtime proof):** **Publication boundary:** [Desktop Pet v0.4.2](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.2) is public (release `412241744`, `2026-10-14T01:09:50Z`, immutable source `cb10ce2c3cc65402cbb914d60301f7627d5546d5`). Six anonymous assets, strict/deep signatures, matched protocol 4 / observation capability 2 / integration API 1 and all 68 deployed resource hashes/exact closure were verified. The neutral formula at tap `32919c745864b54a2e716fcc5a3714d0d38d3e84` and actual public v0.4.1 → v0.4.2 Brew upgrade passed receipt/derivation, five signatures, `brew test` and native UI/control readiness in Auto (Herdr 0 / Tern 0). A fresh public standalone v0.4.1 install and two concurrent installers reused the live owner; native Check offered actual public v0.4.2 and consent completed `standalone_replace`, `manager_exited`, `installed:true`, `applied:true`, with the same config/state and exact public v0.4.2 image ready. Check preserved the locator/profile/lifecycle/locks/PID but changed the `preferences.json` check cache; not all config bytes were unchanged. [Corrected integration-only Herdr v0.4.1](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.4.1) is public (release `412252863`, immutable ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`): two published wrappers reused the same Brew v0.4.2 PID/instance/image without revealing a hidden app; explicit config/state plus a genuine private patched Herdr 0.9.3 socket reached Auto Herdr 1 / Tern 0. Two concurrent fresh published bootstrap scripts installed/reused one OS-home v0.4.2 app without integration-local native files or starting a daemon. This script evidence is not a public Herdr-manager installation receipt. Original Herdr v0.4.0 retains its faulty generated-app/test-scaffold-symlink source tree; it was not integration-only. [Bridge v0.3.7](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.7) is public (release `412267469`, immutable tag `987a4b859f95e922c1c5443ead372d10d0dddb24`), built from exact Core `cb10ce2` and corrected Herdr `6d2ac04`; five anonymous assets, signatures and 68-file closure passed. Only the old stable formula moved to v0.3.7 at tap `657f32cc4cc563ae40ae0432ac5123dcb3f58426`; beta stayed unchanged. Its manual-origin private real-host runtime reached Herdr 1 / Tern 0, not managed A→B acceptance. The actual frozen public Brew v0.3.6 → v0.3.7 A `6a10663c2d2f0177990623a969238af4` and separately consented Core v0.4.2 B `c66961f9e5a91c0023545065ae61385b` completed, including real Brew retirement (exit 0, quiescent); all config-file hashes and 87 old A-evidence files stayed unchanged across B. Actual Core v0.4.2 UI showed Desktop Pet / Herdr Waiting 0 / Tern Disabled 0. In contrast, real managed v0.3.7 A `a1aec5902020de7bcaaf583b1a5f50a5` failed native `/tmp` profile validation: manager exit 1 occurred before checkout replacement/registry registration, and the frozen helper retained Unknown / `manager_exited`, installed/applied false. That VM and its journals/reservation remain preserved without recovery or replay.

**Historical public v0.3.8 managed result; safe successor boundary:** Managed v0.3.6 → public v0.3.8 A completed (`16b6307413ec409efe3eb5a04c9cc21f`), with the original signed v0.3.6 helper confirming completed status. After explicit Check settled A, B preflight rejected `managed source changed; migration never resets user changes`: exactly 68 generated files under `dist/HerdrDesktopPet.app` were untracked. This was before B consent/Stop; no B manager/fault flag was introduced and the historical source PID `3563` stayed unchanged. The immutable original public v0.3.8 tag has exactly 10 regular UTF-8 Git files and no `.gitignore`; its B limitation remains historical fact, not retroactively repaired by public v0.3.9. Do not manually ignore files, `git clean` or reset to bless the old tag. Already-completed managed v0.3.8 users may install a successor only after the retained **matching signed helper** confirms the known terminal old operation, backing up the actual config/state and old evidence, explicit **Stop** of the source, and explicit successor installation by the **real owning Herdr manager**. Never replay an Unknown manager operation; preserve its VM/journals/reservations for reconciliation.

**Historical working-metadata and normal-update proof, not final v0.3.9/v0.4.4 acceptance:** Genuine managed B using working metadata `f7edf5e0eba374248b9a5ee9d66d26109b39eca2` with unchanged public v0.3.8 native bytes and public Core v0.4.3 reached NativeReady/Commit, then definite retirement failure (manager exit 1, quiescent). Only the introduced owned `uchg` flag was removed; independent explicit Resume consent pinned the same plan SHA `3615fa140fd1b0d49910f5f2ef170b64122243c6c65d0098d4ecfd729e951f16`, and attempt 2 completed (real CLI exit 0, quiescent), retiring the old managed app/source while Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8` reused the same v0.4.3 instance. All 87 old A-evidence files, five first-attempt/plan/consent files, old config and unrelated default-profile hashes remained unchanged. The actual public neutral v0.4.2 → v0.4.3 normal update also completed (`81c82d837b65fb03584bfb1e668119c3`, `standalone_replace`), same profile/Herdr 0 readiness and Rubelia UI, without Herdr reinstall. Check changed only `last_update_check`; after Check all config bytes and 87 old A-evidence files remained unchanged across update. Neither historical result proves final public v0.3.9 A/B or a v0.4.4 update.


## Features

- A movable, resizable desktop character with touch and petting reactions, an independent status bubble, and menu-bar controls.
- Observed provider session cards with search, filters, ordering, and distinct local/remote source labels; remote observation is optional and read-only.
- One-line replies to selected **local** agent sessions, with IME-aware input and native status feedback.
- Customizable character, bubble, dialogue, and visibility settings; optional verified character packs and an official catalog browser.
- Integration-owned lifecycle actions, capability-guarded linked-worktree removal, and explicit installation-aware app updates.
- **Published core:** a unified Connection setting selects Auto or an opaque registered provider, shows actual provider status, and saves live changes in one shared profile. Published legacy v0.3.6 does not include this capability-2 UI.

## Requirements

- **Apple Silicon Mac with macOS 13 or later.** Intel Macs, Windows, and Linux are not supported native targets.
- Herdr observation needs an existing compatible local server (0.9.0+ for snapshots); replies separately require `agent.prompt`. Manual native observation does not require a plugin or patched host. Optional client-attach auto-start needs official Herdr 0.9.3 with the [integration-owned patch and host build instructions](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md), or an actual hook-supporting host. Stock Herdr 0.9.0/0.9.3 lack that hook; plugin installation does not patch the host.
- Source builds require Rust/Cargo, Bun, Node.js/npm, and Xcode Command Line Tools; prebuilt installation does not. The app is **ad-hoc signed, not Developer ID signed or notarized**. Gatekeeper may require explicit local approval; signature validation does not imply notarization.

## Installation and optional integrations

The neutral v0.4.4 formula is public at tap `2aa9a4be9517e0232a3f2b8e2b3c748045c69586`. A qualified fresh Brew v0.4.4 install passed (74 files / 44.3 MB and five inner/outer re-signatures); authenticated inspection verified the Homebrew-derived image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312`. Install/inspection created no neutral config/state or standalone app and did not start a daemon. Only explicit manual **Start** produced actual v0.4.4 UI/control readiness, PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470`. Rubelia and actual Settings showed **Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0**; explicit Hidden afterward preserved that default instance. This is fresh-install proof, not a v0.4.4 normal-update or managed-migration result. Installation commands:

```sh
brew install hanbong5938/tap/desktop-pet
desktop-pet --version
desktop-pet start
desktop-pet status
```

The formula owns the app under Homebrew's prefix, not `/Applications`, and puts the CLI on `PATH`. It installs neither host plugin nor patched host. Do not substitute the old Herdr formula as the neutral default.

For prebuilt or source installation use the **core** checkout, not a Herdr integration checkout:

```sh
git clone https://github.com/hanbong5938/desktop-pet.git
cd desktop-pet
# Fixed published v0.4.4 prebuilt; unavailable/invalid downloads never build source:
git checkout --detach v0.4.4
bash scripts/install.sh --prebuilt
```

Default installation is **prebuilt-only**: unavailable downloads or validation failure never silently compile source. A non-Brew prebuilt belongs at the OS account home's `Applications/DesktopPet.app`; explicit source installation uses the core checkout's `dist/DesktopPet.app`. Installation admission may reuse a verified healthy installation rather than take over another owner. Read the reported executable/shared locator instead of assuming the source path is the selected installation. Reservations or live profiles prevent unsafe replacement. See [installation discovery](docs/development.md#profiles-and-installation-discovery).

For explicit **v0.4.4 source installation**, use a separate checkout pinned to the actual public tag:

```sh
git clone https://github.com/hanbong5938/desktop-pet.git desktop-pet-source
cd desktop-pet-source
git checkout --detach v0.4.4
bash scripts/install.sh --source
```

Install only the separate integrations you want:

```sh
herdr plugin install hanbong5938/herdr-desktop-pet
tern plugin install https://github.com/hanbong5938/tern-desktop-pet
```

Tern v0.2.0 is published. Original Herdr v0.4.0 retains its faulty source tree; corrected integration-only [Herdr v0.4.1](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.4.1) is published at immutable `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`. Published wrapper/bootstrap reuse of Core v0.4.2 is verified, not managed migration/retirement acceptance. Plugin updates/removal affect only integrations, never the shared app, profile or characters. Herdr is a thin consumer; Tern never downloads, builds, installs or auto-starts the native app. Native app updates do not reinstall host plugins. Source/link instructions belong to each integration repository; do not link the core as a Herdr plugin.

## Profiles, connection and migration

New profiles default to **Auto** and keep running without any adapter/server. `--host auto|PROVIDER` selects Auto or an opaque registered provider, not a different app or a fixed provider enumeration. Connection displays actual connections, not installation detection. Auto allows simultaneous observation, not provider fallback or deduplication by session name/path.

Profile precedence is **explicit `--config-dir` / `--state-dir` → `DESKTOP_PET_CONFIG_DIR` / `DESKTOP_PET_STATE_DIR` → default-profile record → neutral XDG defaults** (`~/.config/desktop-pet`, `~/.local/state/desktop-pet` when XDG variables are absent). Core does not implicitly inherit host profile environment. The Herdr wrapper alone converts host-owned injected paths into explicit native arguments. Back up and select **both** actual directories for start, commands, status and stop.

```sh
desktop-pet profile show --json
desktop-pet start
desktop-pet status
```

For a registered Herdr adapter, select the verified **backend** socket when necessary:

```sh
desktop-pet --herdr-socket /actual/path/to/herdr-api.sock start
desktop-pet preferences set --observation-host auto
```

`--herdr-socket` is adapter-owned and is never the private `<state-dir>/control.sock`. Core without the Herdr adapter does not normalize or register a Herdr socket. Check the actual running executable/version/profile in `status`, not just `--version`; stop the selected daemon before changing its backend. Live provider selection uses Connection or `preferences set --observation-host auto|PROVIDER` (choose one value).

For Tern, copy the actual executable/config/state triple from native status or Connection into **Configure desktop pet connection**, save, then **Connect to desktop pet**. Discovery is not connection proof. Existing isolated Tern, beta and custom profiles remain in place, never automatically merged or deleted. Tern observes its local window/host inventory with adapter-authorized locality; remote sources remain read-only. Replies use a revalidated one-shot `agents:ask`, never PTY injection or Herdr fallback. Accepted may mean queued, not model delivery/completion; uncertain asks are never replayed.

Core owns common lifecycle persistence and `auto_start` (`on|off`); Herdr owns `exit_with_herdr` and endpoints. When `lifecycle.json` is absent, the registered Herdr owner initializes `exit_with_herdr:true` atomically with the first lifecycle write; an existing file missing that key reads false. Legacy `enabled`, including false, migrates to `auto_start`. Auto/no-adapter runs remain alive; Herdr-only disconnect/disable rules are detailed in [lifecycle guidance](docs/usage.md#lifecycle-and-plugin-actions).

Legacy v0.3.6 is not converted by a rename, alias or formula swap. Follow the explicit [bridge A → neutral B migration](docs/migrations/desktop-pet.md), preserving the same profile and matching signed historical helper for old evidence.

## In-app updates

**Settings → App updates** separates automatic checking (default on), beta opt-in (default off), **Check now**, running version/origin, policy-specific check times and explicit installation-aware actions. Checks never install, restart, refresh Brew metadata or alter Git. Checks do not change lifecycle, installation, running image or selected profile, but successful checks may write the policy-specific timestamp cache (`last_update_check` / `last_beta_update_check`) in `preferences.json`; they are not byte-for-byte read-only. Stable-only checks select official stable releases; beta opt-in selects the highest eligible stable/beta SemVer, with a final release ahead of its same-base beta and no downgrade on opt-out.

Standalone, Homebrew and core-source owners remain distinct; normal native updates do not use a Herdr plugin manager. Source rebuilds use current disk contents without Git pull/reset/ref change. A verified installed candidate can be applied without reinstalling, but **installed is not applied or running**, and source-current/pinned results are not proof of a public release.

Consent and Prepare recheck drafts, IME and pending interactions while the exact original instance still runs. An ownership ACK is not installation or permission to Stop: only matching final visible/prepared ACK after accepted Prepare and a fully delivered accepted reserved Stop authorize replacement. Slow staging stays responsive; the 30-second handoff deadline means waiting, not cancellation. Candidate readiness requires the exact durable image/origin/profile. User Stop forbids restart/rollback start. Unknown manager execution must never be blindly replayed.

The UI retains Previous operation separately from a current check and exposes copyable diagnostics. Use the matching verified helper's status and explicit recovery before retrying. Protocol-4 `updates-v4/` evidence coexists with immutable old journals; never reserialize old protocol-1/2/3 evidence or recover it with the v4 helper. See [update behavior and recovery](docs/usage.md#in-app-updates) and the [CLI contract](docs/cli.md#app-update-status-and-recovery).

## Controls

| Action | Result |
| --- | --- |
| Tap head/body; move across head | React; pet Rubelia. |
| Drag character; use bottom-right grip | Move; resize the character. |
| Drag bubble background or an edge/corner | Move attached character or standalone bubble; resize bubble independently. |
| Left-click menu-bar icon | Open Settings for characters, observation, bubble and lifecycle. |
| Expand bubble, select a local agent card | Open a one-line reply; Enter, Command+Enter or Send submits. Remote cards are read-only. |

IME composition **never submits**; Escape cancels composition first. Search-field Enter/Command+Enter never sends a reply. Character and bubble visibility are independent; hiding both or enabling full-window click-through can make them inaccessible by clicking the desktop. Use the menu-bar icon's **Show and Enable Character** recovery action when needed (the default mode always shows the icon; recovery-only mode shows it when necessary). For full controls, search, saved-machine remote observation and visibility behavior, see the [usage guide](docs/usage.md#controls).

## Optional characters and outfits

The [character gallery](https://hanbong5938.github.io/herdr-characters/) previews optional packs; the [separate repository](https://github.com/hanbong5938/herdr-characters) holds their source and terms. In Settings, choose **Characters → Browse Characters…**; browsing portraits does not download archives. Eligible published official packs have an explicit verified **Download & Apply**. For a local folder or `.herdrchar`, **Import** alone does not activate it: select its installed card/revision and click **Apply**. Rubelia remains the only bundled/default character. See [character packs](docs/usage.md#character-packs) and [authoring](docs/development.md#character-authoring).

## Safety and limitations

- Replies require the selected source's **local prompt capability**. Herdr uses `agent.prompt`; Tern uses one-shot `agents:ask`, never SSH, a shell or raw pane input. Remote cards cannot send. Acceptance is **not agent completion**. Handle approval/questions in the actual terminal and inspect uncertain delivery before any manual resend. Herdr's submission API has a current-occupant race, not an atomic session guarantee. [Delivery details](docs/usage.md#inline-replies-and-message-delivery).
- **Remove Worktree…** is irreversible: confirm the **exact checkout path** and the **entire workspace's** tabs/panes before proceeding. It terminates their processes/agents and removes the linked checkout **including ignored files**; no Trash/undo. It does not delete the main repository or branch. [Removal safeguards](docs/usage.md#worktree-removal).
- Remote observation requires selected saved-machine profiles and a compatible remote Herdr server, **not** a remote pet installation. The local app polls remote state and cannot recover missed brief transitions or send remote prompts. [Observation guide](docs/usage.md#observation-sources).
- Automatic app-update checks preserve lifecycle/installation/image/profile but may write the successful-check timestamp cache in `preferences.json`; installing or applying an update requires explicit consent. An updater ownership ACK is **not completion**. For an unknown result, check [in-app update recovery](docs/usage.md#in-app-updates) and [CLI update status and recovery](docs/cli.md#app-update-status-and-recovery) before retrying.

## Configuration and troubleshooting

The selected config directory holds `preferences.json`, `lifecycle.json`, managed `characters/` and `menu-bar-icons/`; state holds the process lock, private control socket, daemon `desktop-pet.log` and updater evidence. Use the exact precedence above and back up both actual directories before upgrades or migration.

Hidden windows are not an exited daemon: use menu-bar recovery instead of reinstalling. Auto/no-adapter keep running without a host; only Herdr-only mode applies its adapter-owned disconnect policy. For managed auto-start inspect actual `client.attached` support, `auto_start`, daemon executable/profile and logs. Verify the actual Herdr backend and adapter-owned `--herdr-socket`; remote errors require saved-machine/forwarding diagnostics. Launch on the **Mac whose desktop should display the pet**. Download failure never falls back to source; use explicit `--source` only for source development. [Troubleshooting](docs/usage.md#configuration-and-troubleshooting).

## Documentation and websites

[Desktop Pet Wiki](https://github.com/hanbong5938/desktop-pet/wiki) is the core-owned generated reading view; reviewed source remains authoritative. Live Wiki publication is separately verified. The old Herdr site is an integration/migration entrance, not a second app site.

- [Usage and recovery](docs/usage.md) · [development and installation](docs/development.md) · [native automation CLI contract](docs/cli.md#english) · [Herdr-owned host integration](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md) · [Tern integration](https://github.com/hanbong5938/tern-desktop-pet).
- [Release history](docs/releases/README.md) · [v0.3.6 changes](docs/releases/v0.3.6.md) · [v0.3 upgrade guidance](docs/releases/0.3.md#upgrade-from-v021-or-beta) · [historical beta/preview guidance](docs/migrations/unreleased.md) · [v0.1.11 → v0.2.0 migration](docs/migrations/v0.1.11-to-v0.2.0.md).
- Planned promotional [English](https://hanbong5938.github.io/desktop-pet/en/), [Korean](https://hanbong5938.github.io/desktop-pet/), [Simplified Chinese](https://hanbong5938.github.io/desktop-pet/zh-Hans/), and [Japanese](https://hanbong5938.github.io/desktop-pet/ja/) Pages describe this **native** app, not a browser runtime; deployment is not established here. [GitHub Pages guide](docs/development.md#website-github-pages). The separate [character gallery](https://hanbong5938.github.io/herdr-characters/) previews optional packs in English and Korean.

## Artwork and licensing

[Software license](LICENSE.txt) and artwork/model terms are **distinct**: MIT software terms do not relicense character art. Bundled Rubelia's [license](assets/rubelia-default/LICENSE.txt), [attribution](assets/rubelia-default/ATTRIBUTION.txt), [source record](assets/rubelia-default/source-record.json), and [background-repair record](assets/rubelia-default/background-repair.json) remain authoritative. `LICENSE.txt` also retains the original Coding Cat notice; its artwork/source moved to the [character repository](https://github.com/hanbong5938/herdr-characters). Keep each optional pack's own terms with it. Artwork-owner approval does not grant model rights; Qwen's research license is an authoring record, and model engines/weights are not bundled. See [character authoring and licenses](docs/development.md#character-authoring).
