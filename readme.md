# Desktop Pet — Herdr integration

[English](readme.md) · [한국어](readme.ko.md) · [日本語](readme.ja.md) · [简体中文](readme.zh-Hans.md)

**Publication boundary:** legacy [Herdr v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6) is shipped with protocol 3 / observation capability 1. Independent Desktop Pet v0.4.0 (protocol 4 / observation capability 2 / integration API 1) and the legacy bridge v0.3.7 are reviewed candidates, not verified public installations. Commands below describe the neutral cutover; do not treat notes, a source version or a tag as publication proof.

Install only the integrations you want. Herdr is a thin consumer; Tern does not download, build, install or auto-start the native app. Plugin updates/removal affect integrations only, never the shared app, profile or characters. Native updates do not reinstall Herdr plugins.

[Desktop Pet / core app](https://github.com/hanbong5938/desktop-pet/blob/main/readme.md) · [Herdr host / patch / OMP / link](integrations/herdr/README.md) · [Legacy A → B migration](https://github.com/hanbong5938/desktop-pet/blob/main/docs/migrations/desktop-pet.md)

After the neutral release and formula are actually published, the public command is:

```sh
brew install hanbong5938/tap/desktop-pet
herdr plugin install hanbong5938/herdr-desktop-pet
herdr plugin action invoke start --plugin desktop-pet
```

Apple Silicon Mac, macOS 13+. Source builds require Rust/Cargo, Bun, Node.js/npm and Xcode Command Line Tools; prebuilt installation does not. The app is ad-hoc signed, not Developer ID signed or notarized. Herdr snapshots need a compatible local server (0.9.0+); replies separately need `agent.prompt`. Optional client-attach auto-start still needs [the integration-owned patch and host instructions](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md) on official Herdr 0.9.3 or an actual hook-supporting host. Stock 0.9.0/0.9.3 lack that hook; plugin installation does not patch the host.

For prebuilt or source installation, use the **core** checkout, not a Herdr integration checkout. Default installation is prebuilt-only; download or validation failure never silently compiles source. `--source` explicitly builds only the current core checkout and uses its `dist/DesktopPet.app`.

> Native app features below are owned and documented by the independent core, not built in this integration checkout.

## Features

- A movable, resizable desktop character with touch and petting reactions, an independent status bubble, and menu-bar controls.
- Observed provider session cards with search, filters, ordering, and distinct local/remote source labels; Herdr remote observation is optional, and remote cards are read-only.
- One-line replies to selected **local** agent sessions, with IME-aware input and native status feedback.
- Customizable character, bubble, dialogue, and visibility settings; optional verified character packs and an official catalog browser.
- Herdr lifecycle actions, guarded linked-worktree removal, and explicit installation-aware app updates.
- **Current source only:** a unified Connection setting selects Auto or a registered provider (including Herdr/Tern), shows actual provider status, and saves live changes in one shared profile; published v0.3.6 does not include this capability-2 UI.

### Tern observation: source-only exchange v2

When Auto selects Tern (or Tern is selected explicitly), current exchange-v2 source observes all connected Tern SDK hosts automatically, not just local agents. Cards carry host names and Local/Remote labels; each host has independent Offline status and observation lifetime. Tern host observation is separate from Herdr's local/remote/machines scopes and needs no new host-selection UI. Replies remain local-only; remote cards never accept replies.

This is not published v0.3.6 behavior; its historical local-only Tern path is unchanged. Real remote SDK agents and a matching native/plugin installation are **not yet verified**. The current plugin's neutral `DesktopPet.app` installation requirements do not establish compatibility with this source branch's `HerdrDesktopPet.app` bundle. See [usage](docs/usage.md) and [development / evidence](docs/development.md) for the full source contract and verification boundaries.


## Source / link

Install or build the core first (`bash scripts/install.sh --source` in the **desktop-pet core** checkout). Then link **this integration checkout**, not the core. `plugin link` does not run a build. The integration wrapper discovers a verified compatible native executable, preserving the exact profile/socket provenance; it never compiles a native app here. Client-attach startup remains optional and requires the actual patched host/hook; plugin install does not patch Herdr. See [separate checkouts](integrations/herdr/README.md#build-in-separate-checkouts).

## Ownership

This repository owns `native-adapter`, `herdr-plugin.toml`, thin install/run wrappers, `integrations/herdr` and `integrations/omp`, integration validation, and frozen historical releases. App/UI/rig/artwork/packaging/source instructions belong to [desktop-pet](https://github.com/hanbong5938/desktop-pet). `.herdrchar`, pack v5, `default@0`, rig ABI and [herdr-characters](https://github.com/hanbong5938/herdr-characters) catalog/gallery authority are unchanged. Historical [v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6) and older release bodies/tags/assets remain immutable; an integration-only v0.4.0 does not carry a native archive.

## Artwork and licensing

[Software license](LICENSE.txt) and artwork/model terms are **distinct**: MIT software terms do not relicense character art. Bundled Rubelia's [license](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/LICENSE.txt), [attribution](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/ATTRIBUTION.txt), [source record](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/source-record.json), and [background-repair record](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/background-repair.json) remain authoritative. `LICENSE.txt` also retains the original Coding Cat notice; its artwork/source moved to the [character repository](https://github.com/hanbong5938/herdr-characters). Keep each optional pack's own terms with it. Artwork-owner approval does not grant model rights; Qwen's research license is an authoring record, and model engines/weights are not bundled. See [character authoring and licenses](https://github.com/hanbong5938/desktop-pet/blob/main/docs/development.md#character-authoring).
