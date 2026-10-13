# Desktop Pet

An independent native macOS desktop companion. Rubelia reacts to touch, displays sessions from registered providers such as Herdr and Tern, and lets you reply when that source grants prompt capability. The app also runs without a host.

**English** · [한국어](readme.ko.md) · [简体中文](readme.zh-Hans.md) · [日本語](readme.ja.md) · [Release history](docs/releases/README.md) · [Core releases](https://github.com/hanbong5938/desktop-pet/releases)

<img src="assets/rubelia-thumbnail.png" alt="Rubelia, the default desktop companion" width="220">

[Browse the optional character and outfit gallery](https://hanbong5938.github.io/herdr-characters/) (opens in English; choose KO for Korean) · [Character pack source and licenses](https://github.com/hanbong5938/herdr-characters). **Only Rubelia (`default@0`) is bundled.**

**Publication boundary:** independent [Desktop Pet v0.4.0](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.0) and `hanbong5938/tap/desktop-pet` are published and were installed from public assets in a disposable Mac. Current source targets separately approved **v0.4.1**, whose fixed prebuilt assets are not yet published; use the immutable v0.4.0 checkout for its installer. Bridge v0.3.7 and integration tags remain unpublished, and the final acceptance matrix remains incomplete. Legacy Herdr v0.3.6 and published neutral v0.4.0 tags/assets remain immutable.

## Features

- A movable, resizable desktop character with touch and petting reactions, an independent status bubble, and menu-bar controls.
- Observed provider session cards with search, filters, ordering, and distinct local/remote source labels; remote observation is optional and read-only.
- One-line replies to selected **local** agent sessions, with IME-aware input and native status feedback.
- Customizable character, bubble, dialogue, and visibility settings; optional verified character packs and an official catalog browser.
- Integration-owned lifecycle actions, capability-guarded linked-worktree removal, and explicit installation-aware app updates.
- **Current candidate:** a unified Connection setting selects Auto or an opaque registered provider, shows actual provider status, and saves live changes in one shared profile. Published legacy v0.3.6 does not include this capability-2 UI.

## Requirements

- **Apple Silicon Mac with macOS 13 or later.** Intel Macs, Windows, and Linux are not supported native targets.
- Herdr observation needs an existing compatible local server (0.9.0+ for snapshots); replies separately require `agent.prompt`. Manual native observation does not require a plugin or patched host. Optional client-attach auto-start needs official Herdr 0.9.3 with the [integration-owned patch and host build instructions](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md), or an actual hook-supporting host. Stock Herdr 0.9.0/0.9.3 lack that hook; plugin installation does not patch the host.
- Source builds require Rust/Cargo, Bun, Node.js/npm, and Xcode Command Line Tools; prebuilt installation does not. The app is **ad-hoc signed, not Developer ID signed or notarized**. Gatekeeper may require explicit local approval; signature validation does not imply notarization.

## Installation and optional integrations

The published neutral Homebrew installation commands are:

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
# Requires this checkout's exact published core assets (current 0.4.1 is not yet published):
bash scripts/install.sh --prebuilt
# Explicit source alternative for the current core checkout:
bash scripts/install.sh --source
```

Default installation is **prebuilt-only**: unavailable downloads or validation failure never silently compile source. A non-Brew prebuilt belongs at the OS account home's `Applications/DesktopPet.app`; explicit source installation uses the core checkout's `dist/DesktopPet.app`. Installation admission may reuse a verified healthy installation rather than take over another owner. Read the reported executable/shared locator instead of assuming the source path is the selected installation. Reservations or live profiles prevent unsafe replacement. See [installation discovery](docs/development.md#profiles-and-installation-discovery).

Install only the separate integrations you want:

```sh
herdr plugin install hanbong5938/herdr-desktop-pet
tern plugin install https://github.com/hanbong5938/tern-desktop-pet
```

These commands describe the integration cutover; integration tags are not yet published. Plugin updates/removal affect only integrations, never the shared app, profile or characters. Herdr is a thin consumer; Tern never downloads, builds, installs or auto-starts the native app. Native app updates do not reinstall host plugins. Source/link instructions belong to each integration repository; do not link the core as a Herdr plugin.

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

**Settings → App updates** separates automatic checking (default on), beta opt-in (default off), **Check now**, running version/origin, policy-specific check times and explicit installation-aware actions. Checks are read-only: they never install, restart, refresh Brew metadata or alter Git. Stable-only checks select official stable releases; beta opt-in selects the highest eligible stable/beta SemVer, with a final release ahead of its same-base beta and no downgrade on opt-out.

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
- Automatic app-update checks are read-only; installing or applying an update requires explicit consent. An updater ownership ACK is **not completion**. For an unknown result, check [in-app update recovery](docs/usage.md#in-app-updates) and [CLI update status and recovery](docs/cli.md#app-update-status-and-recovery) before retrying.

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
