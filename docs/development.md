# Desktop Pet

An independent native macOS desktop companion. Rubelia reacts to touch, displays sessions from registered providers such as Herdr and Tern, and lets you reply when that source grants prompt capability. The app also runs without a host.

[English](../readme.md) · [한국어](../readme.ko.md) · [日本語](../readme.ja.md) · [简体中文](../readme.zh-Hans.md)

**Publication boundary:** [Core v0.4.1](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.1) and its neutral formula at public tap `47df565` are published. All six anonymous assets, signatures, matched 4/2/1 capabilities and the 68-file deployed closure were verified. Actual disposable-Mac v0.4.0 installation and v0.4.0 → v0.4.1 Brew upgrade preserved the profile; receipt, physical root, derivation, `brew test`, signatures and same-profile native UI/control readiness were verified. Same-config/different-state startup was rejected before preferences writes. Fresh anonymous standalone v0.4.1 installation/native readiness was also verified, not SDK account proof. Herdr v0.4.0 is public but its immutable generated app/failed-test files are rejected by the corrected bridge guard. Tern v0.2.0 is public with its compatible retained Core v0.4.1 pin. The approved [Core v0.4.2 repair candidate](releases/v0.4.2.md) and corrected Herdr v0.4.1 are not yet public. Bridge v0.3.7 must use those exact newly published sources; publication and migration/two-host acceptance remain pending, with actual Tern keychain/provider prerequisites blocked. Historical Herdr v0.3.6 (3/1), neutral v0.4.0/v0.4.1 tags, bodies, assets and evidence remain immutable.

## Requirements

Apple Silicon Mac, macOS 13+. Source builds require Rust/Cargo, Bun, Node.js/npm and Xcode Command Line Tools; prebuilt installation does not. The app is ad-hoc signed, not Developer ID signed or notarized. Herdr snapshots need a compatible local server (0.9.0+); replies separately need `agent.prompt`. Optional client-attach auto-start still needs [the integration-owned patch and host instructions](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md) on official Herdr 0.9.3 or an actual hook-supporting host. Stock 0.9.0/0.9.3 lack that hook; plugin installation does not patch the host.

## Installation and optional integrations

The published neutral v0.4.1 is available through Homebrew:

```sh
brew install hanbong5938/tap/desktop-pet
desktop-pet --version
desktop-pet start
desktop-pet status
```

For prebuilt or source installation, use the **core** checkout, not a Herdr integration checkout. Default installation is prebuilt-only; download or validation failure never silently compiles source. `--source` explicitly builds only the current core checkout and uses its `dist/DesktopPet.app`.

```sh
git clone https://github.com/hanbong5938/desktop-pet.git
cd desktop-pet
# Published immutable v0.4.1 installer and pinned assets:
git checkout --detach v0.4.1
bash scripts/install.sh --prebuilt
# Explicit v0.4.2 candidate source development uses a separate checkout (below).
# Replace with the installer-reported executable and your actual profile paths:
/absolute/reported/desktop-pet --config-dir /absolute/dev/config --state-dir /absolute/dev/state start
```

For explicit **v0.4.2 candidate development** (prebuilt assets are not yet public), use a separate current-source checkout:

```sh
git clone https://github.com/hanbong5938/desktop-pet.git desktop-pet-source
cd desktop-pet-source
bash scripts/install.sh --source
```

Default and `--prebuilt` from immutable v0.4.0/v0.4.1 checkouts use their respective published pinned assets. The current v0.4.2 candidate has no public prebuilt assets yet: use immutable v0.4.1 for public installation. Failure never silently builds source; only explicit `--source` builds current core. A standalone prebuilt installs to the **OS account home**’s `Applications/DesktopPet.app`; admitted source uses that core checkout’s `dist/DesktopPet.app`. Admission may reuse a verified healthy owner: use the installer-reported executable, not an assumed dist path. The private atomic shared locator is the account home’s `.local/share/desktop-pet/installation.json`; it is discovery evidence, not authority. Invalid explicit paths/locators fail closed, and executable/signature/capability/origin are reverified. Homebrew owns its actual formula/prefix; no installer or integration silently takes over a live/reserved installation.

Install only the integrations you want. Herdr is a thin consumer; Tern does not download, build, install or auto-start the native app. Plugin updates/removal affect integrations only, never the shared app, profile or characters. Native updates do not reinstall Herdr plugins.

```sh
herdr plugin install hanbong5938/herdr-desktop-pet
tern plugin install https://github.com/hanbong5938/tern-desktop-pet
```

## Profiles, connection and migration

New profiles default to Auto. Connection lists registered providers and their actual connections, not installation detection; provider IDs are opaque and capabilities, not source-name prefixes or paths, authorize operations. Auto is simultaneous observation, not provider fallback. Remote sources are read-only; Tern prompts do not permit worktree removal. Submission acceptance is not agent completion.

See [current usage](usage.md), [profile/discovery contract](development.md#profiles-and-installation-discovery), and [legacy migration](migrations/desktop-pet.md) for exact defaults, precedence, adoption and A→B migration. Copy the actual executable/config/state triple from status or Connection into Tern Configure, then Connect. Existing isolated Tern/beta/custom profiles are preserved in place, never merged.

```sh
desktop-pet profile show --json
desktop-pet --herdr-socket /actual/path/to/herdr-api.sock start
desktop-pet preferences set --observation-host auto
```

## Profiles and installation discovery

The neutral product owns `DesktopPet.app` (`io.github.hanbong5938.desktop-pet`), CLI `desktop-pet`, helper `desktop-pet-update-coordinator`, release authority `hanbong5938/desktop-pet` and formula `hanbong5938/tap/desktop-pet`. A non-Brew prebuilt installs to the OS account home’s `Applications/DesktopPet.app`; source builds stay in the exact core checkout’s `dist/DesktopPet.app`. No permanent old-name alias is a normal discovery route.

Profile selection is **explicit `--config-dir` / `--state-dir` > `DESKTOP_PET_CONFIG_DIR` / `DESKTOP_PET_STATE_DIR` > the default profile record > neutral XDG defaults**. Defaults are `${XDG_CONFIG_HOME:-$HOME/.config}/desktop-pet` and `${XDG_STATE_HOME:-$HOME/.local/state}/desktop-pet`. The default config root contains `default-profile.json`: `{ "version":1, "config_dir":"/absolute/config", "state_dir":"/absolute/state" }`. It atomically records both real data locations; recursive profile pointers are forbidden. `desktop-pet profile show --json` is read-only: it neither starts a daemon nor creates directories.

Migration adopts old directories **in place**. Keep preferences, lifecycle, characters, menu-bar-icons, lock/control socket and existing `updates/` journals/helpers exactly where they are; never move, merge or delete profiles. A record is written only when connecting the legacy base profile to an otherwise unclaimed neutral default. An existing default pointing elsewhere remains unchanged; use explicit paths for the migrated profile. Existing separate Tern, beta and custom profiles remain separate. To deliberately choose another default:

```sh
desktop-pet profile adopt --config-dir /absolute/config --state-dir /absolute/state
```

Adoption records the pair, not a data copy. Active updates/reservations, another live daemon, invalid/symlink/nonowned directories fail without mutation. Always back up and select **both** directories, and use that same pair for start, status, commands and stop. Core does not interpret `HERDR_PLUGIN_*` as app defaults: the Herdr wrapper alone converts its injected profile precedence into explicit native arguments. Tern supplies its validated pair explicitly and sets matching neutral environment values. Status / Connection shows the actual executable and selected profile.

The shared locator is the **OS account home’s** `.local/share/desktop-pet/installation.json`: `{ "version":1, "executable":"/absolute/path/to/desktop-pet", "kind":"homebrew" }`; `kind` is exactly `homebrew`, `standalone` or `source`. It is atomically written under an install lock in the same directory. Consumers discover **user-specified absolute executable → valid locator → PATH `desktop-pet` → actual `brew --prefix desktop-pet` executable → `~/Applications/DesktopPet.app`**. Invalid explicit paths or locators fail closed, not silently fall through. The locator is not authority: private ownership/regular-file checks, the actual executable/bundle/signature, capabilities and origin must be verified every time. A running image/profile mismatch is displayed rather than silently starting a different installation. Compatible same/newer stable installations may be reused under the lock; stable installation never silently replaces beta/incompatible apps or takes over another valid source locator. Live installations need Stop/update reservation before replacement; failed staged bytes never delete the existing app.

Concurrent bootstrap installs converge on the verified winner: selection is refreshed only after the legacy→neutral global gates are held, its actual physical root and the requested root are fenced in sorted order, then the locator install lock is acquired. A newly selected root outside that lockset permits one gate-retained root refresh, with the install lock released before rebuilding root fences and fresh verification afterward; it is not an installer/manager replay. Healthy same/newer stable reuse does not replace the app or rewrite an existing locator and remains possible if a daemon has attached to the winner. Reservations still reject reuse in both namespaces, and actual replacement still requires every affected installation/profile to be inactive. Invalid/incompatible selections and a lower-version different owner fail closed.

New observation defaults to `Auto`; `--host PROVIDER` selects an opaque registered provider, not an app variant. Observation precedence is explicit CLI > `DESKTOP_PET_OBSERVATION_MODE` > saved `observation.host` > new-profile Auto. Old saved `auto/herdr/tern` selections remain; old `both` becomes `auto` and old host-less settings retain Herdr **only as legacy read migration**, not new accepted mutation values. Provider connection counts come from registered adapters; control remains the private same-UID profile socket. Capabilities and locality authorize operations, never source prefixes or filesystem paths. Generic core settings coexist with adapter-owned lifecycle/preferences keys under the same atomic profile writer; `exit_with_herdr` and endpoints remain Herdr-owned. Auto/zero-adapter/Tern-only runs do not acquire a Herdr socket or exit just because Herdr disappears.

`herdr plugin install` and `tern plugin install URL --force` update integrations only. Herdr uninstall/unlink and Tern remove/unlink preserve the shared app, profile and characters. Normal app updates never reinstall host plugins. Tern Connect/Configure only validates/saves the connection and attaches to an explicitly started pet: it does not install, build or auto-start it. A legacy saved executable may be proposed for replacement with the verified neutral executable and **the existing pair of directories**; only a successful user Configure save changes the full triple. Cancel/failure retains the previous connection and offered/uncertain prompts are never replayed.

Core owns common lifecycle persistence and `auto_start` (default on); registered integrations own their extra keys. The Herdr owner initializes `exit_with_herdr:true` **only when `lifecycle.json` is absent**, atomically in the shared profile writer on its first lifecycle write. An existing lifecycle file missing that key reads false; explicit false remains false. Legacy boolean `enabled`, including false, migrates to `auto_start`. Endpoints, startup metadata and unrelated owner fields survive; these settings live in `lifecycle.json`, not `preferences.json`. Bad JSON/values error rather than reset defaults, and failed saves preserve saved/runtime policy. Auto/no-adapter remain alive without Herdr; the following Herdr endpoint/disconnect rules apply only to Herdr-only selection.

`settings set auto_start on|off` accepts exactly `on|off`; extra lifecycle keys are registered by their integration owner. Auto/no-adapter remain alive without a Herdr server. `--herdr-socket ABS` is interpreted only by a registered Herdr adapter, never by neutral core as a control socket.

## Native build and signing

After source installation, the native checks are:

```sh
bun run check
bun run test:native
```

Native hit-overlay blending uses typed subexpressions for compatibility with the release runner's Xcode 16.4 Swift compiler. Preserve these type boundaries when changing the overlay; source builds still require Xcode Command Line Tools.

Debug and release native builds require `codesign`. They ad-hoc sign **only** `rig-decode-worker` with `com.apple.security.cs.allow-jit=true` from [`native/rig/RigDecodeWorker.entitlements.plist`](../native/rig/RigDecodeWorker.entitlements.plist). Cargo tracks plist changes to rebuild and re-sign the worker. JavaScriptCore can JIT-compile the trusted decoder bundle, while the main app and native rig library receive **no** JIT entitlement; worker isolation, validation, and resource limits remain. `bun run package:native` reapplies the worker-only entitlement with the selected signing identity and verifies effective entitlements and strict worker/app signatures after outer-app signing. Failure to sign or verify **stops packaging**. Default ad-hoc packaging is local use, not Developer ID signing or notarization.

The rig bridge checks version and seven C layout sizes **before** handing typed asset/token inputs across the Rust/Swift boundary; initial motion remains borrowed sealed bytes copied by the native host, never a path lookup. The Metal renderer prepasses actual eye-white apertures with independent L/R/unsided stencil bits before stable-ordered color drawing in the same encoder. The CPU semantic hit and raster reference use matching ownership; no-white rigs keep their legacy iris rendering. A `RIG_PROBE_FAULTS` probe can run `--eye-stencil-checks --output ABS` to save real-pixel and semantic fixtures; this does not establish the cause of earlier cancelled/disconnected visual observations.

`scripts/build-rig-native.mjs` records bounded source/compiler/output hashes in `native/target/rig-native/release/rig-native.json`; packaging verifies source and output closure, embeds that build record at `Contents/Resources/rig-native-build.json`, preserves the independent update coordinator and checks its protocol/signature, then writes `dist/DesktopPet.app.rig-native.json` after signing with hashes of **all** deployed `Contents` files. The sidecar stays outside the signed app to avoid a self-hash cycle. The release workflow is configured to upload a separately named `DesktopPet-v0.4.0-rig-native.json` asset alongside the unchanged app-only archive and its two checksum files; neither updater nor installer needs a new archive layout. Compare sidecar hashes to actual unpacked, post-signing app files before asserting deployed-byte provenance.

## Repository layout

| Path | Purpose |
| --- | --- |
| [`native/contracts/`](../native/contracts/), [`native/core/`](../native/core/), [`native/app/`](../native/app/), [`native/update-coordinator/`](../native/update-coordinator/) | Neutral contracts, runtime, composition root and helper; adapters live in the integration repositories |
| [`native/`](../native/) | Rust app, native UI, session handling and character management |
| [`native/rig/`](../native/rig/), [`web/rig/`](../web/rig/) | Rig rendering, decoding and authoring support |
| [`assets/`](../assets/) | Bundled Rubelia default and menu thumbnail only |
| [`tools/`](../tools/) | Generic character authoring and rig inspection utilities |
| [`scripts/`](../scripts/) | Installation, native builds and app packaging |
| [`.github/workflows/release.yml`](../.github/workflows/release.yml) | Release packaging workflow |

## Native localization

Native UI strings live in the static catalog [`native/core/src/i18n.rs`](../native/core/src/i18n.rs), with Simplified Chinese and Japanese catalogs in [`native/core/src/i18n/zh_hans.rs`](../native/core/src/i18n/zh_hans.rs) and [`native/core/src/i18n/ja.rs`](../native/core/src/i18n/ja.rs). Add each new UI message to every catalog rather than translating runtime values, paths, IDs, or pack-owned metadata. Preferences persist the canonical tokens `system|ko|en|zh-Hans|ja`; dialogue overrides accept `ko|en|zh-Hans|ja`, with independent drafts per locale. Keep CLI/patch validation strict while preserving the tolerant fallback for older saved preferences. `system` checks preferred OS languages in order: Japanese tags match case-insensitively; `zh`, explicit Hans and CN/SG regions resolve to Simplified Chinese, but explicit Hant or TW/HK/MO without Hans must be skipped for the next supported language (default English). External catalog metadata remains Korean for `ko` and English fallback for `en`, `zh-Hans` and `ja`; authored dialogue and its fallback belong to each pack.

When changing native text or CLI language contracts, keep the [CLI contract](cli.md), [English](../readme.md), [한국어](../readme.ko.md), [简体中文](../readme.zh-Hans.md) and [日本語](../readme.ja.md) READMEs, and the [English](usage.md), [한국어](usage.ko.md), [简体中文](usage.zh-Hans.md) and [日本語](usage.ja.md) usage guides aligned. These links describe the native app; maintain the promotional website's four translations separately.

## Character authoring

Use the [character creator guide](../.agents/skills/character-creator/SKILL.md) and [`tools/character-pack.py`](../tools/character-pack.py) to author/validate packs. The packaged app includes these resources at `Contents/Resources/creator/SKILL.md` and `Contents/Resources/creator/character-pack.py`. In a source checkout the [local Qwen + See-through production skill](../.agents/skills/create-pet-character/SKILL.md) produces ten independent poses **from an approved illustration**; external engines and model weights are not bundled. Rubelia is the bundled rig template. PNG templates, Aurora, Coding Cat, optional Rubelia outfits and their artwork generators are maintained in [herdr-characters](https://github.com/hanbong5938/herdr-characters); supply an external template with `--templates-root PATH` rather than assuming those assets are bundled.

Keep each imported pack's license, attribution and source-term files with it. Bundled Rubelia's [license](../assets/rubelia-default/LICENSE.txt), [attribution](../assets/rubelia-default/ATTRIBUTION.txt), [source record](../assets/rubelia-default/source-record.json), and [background-repair record](../assets/rubelia-default/background-repair.json) remain the artwork authorities. [`LICENSE.txt`](../LICENSE.txt) retains the original Coding Cat notice; Coding Cat and its drawing source moved to the separate character repository. These notices do not give every repository component a blanket license. Artwork-owner approval does not grant model rights and software MIT does not relicense artwork. Qwen's research license remains an authoring record; no model weights or engines are bundled. Version-specific redraws, face corrections and historical limitations are documented in [v0.1.6](releases/v0.1.6.md), [v0.1.9](releases/v0.1.9.md), and [v0.1.10](releases/v0.1.10.md).

## Website (GitHub Pages)

The website currently uses static images for its product visuals; no video or MP4 asset is required for this local Pages workflow.

Before publishing captures, inspect the rendered pixels: use an isolated demo profile and include only the app, never real session content, account names, local paths, hostnames, email addresses or credentials. Public PNGs must contain only image/color chunks; `check` and `stage` reject EXIF, text, profiles and other metadata. This metadata gate does not inspect visible text, so manual privacy review remains required.

The Korean-default promotional website is in [`site/`](../site/); it describes the native macOS app, not an app running in the browser. Planned core Pages URLs (deployment not verified here): [Korean](https://hanbong5938.github.io/desktop-pet/) · [English](https://hanbong5938.github.io/desktop-pet/en/) · [Simplified Chinese](https://hanbong5938.github.io/desktop-pet/zh-Hans/) · [Japanese](https://hanbong5938.github.io/desktop-pet/ja/). Keep all four page translations and navigation links aligned when editing product copy, with reciprocal `hreflang` links for `ko`, `en`, `zh-Hans`, `ja` and `x-default` pointing to the Korean root; each page also needs its own canonical URL and matching Open Graph locale. The external [character gallery](https://github.com/hanbong5938/herdr-characters) remains separate and Korean/English only. Historical v0.3.2 screenshots must not be presented as current or translated screenshots; sharing an English social preview image does not change the page's language. Headings use a compact responsive scale and tighter spacing; body text, terminal commands and control sizes remain independent of heading scale. From the repository root:

```sh
bun test tools/pages.test.ts
node tools/pages.mjs check
node tools/pages.mjs serve --port 4173
# Open http://127.0.0.1:4173/desktop-pet/ (English: /desktop-pet/en/; Simplified Chinese: /desktop-pet/zh-Hans/; Japanese: /desktop-pet/ja/)
node tools/pages.mjs stage /tmp/herdr-pages
```

The site CLI uses **Node.js 22**; **Bun 1.4.2** is only the HTTP regression-test runner, launching the actual Node server. CI runs this regression before staging. `check` validates local links, anchors, locale metadata and assets, **not** external links over the network. The local preview serves only regular public files; if the final `404.html` fallback is missing, a symlink or nonregular, it returns plain-text `500` with `nosniff` rather than streaming it, and `HEAD` has no body. `stage` checks first and copies only public `site/` files to a **new or empty directory outside the repository**; it does not deploy. The Pages workflow checks PRs and attaches a review artifact without deployment. Pages source is **GitHub Actions**; only eligible `main` runs deploy (merge changes to `main` or manually run the workflow on `main`). These local commands do not change Release binaries, the Wiki or repository Pages settings.
