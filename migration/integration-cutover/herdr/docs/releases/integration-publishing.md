# Publishing the Herdr integration after ownership cutover

This is the **integration-only** publication procedure. The Herdr repository owns `native-adapter`, the Herdr plugin consumer/hooks/attach patch and the OMP outcome producer. `hanbong5938/desktop-pet` owns the common native app, UI, artwork, installer, packages and native releases. Root JavaScript tooling has no product version; adapter crate, Herdr plugin and OMP wire contracts are independent. Only the integration manifest determines an integration tag. The locked adapter resolves its own exact published contracts revision; it does not build the app.

The candidate is source implementation, not publication evidence. It deliberately contains no `core-release.json`, invented digest, installer asset or release ID. Verification can run before a real core publication; integration release checking/publication cannot. Historical release notes and public releases, including v0.3.6 and beta, remain frozen. The separate v0.3.7 bridge is not publishable by this workflow.

## Candidate closure and exact application paths

Candidate root in the core repository: `migration/integration-cutover/herdr/`. All paths in the following list are relative to that root; copy them to the same relative paths in the final **Herdr integration** checkout after the parent completes provenance capture and ownership removal:

- `package.json`, `bun.lock`, `tsconfig.json`, `.gitignore` (replace the old root common-product tooling files together).
- `.github/workflows/integration.yml` (add integration verification).
- `.github/workflows/release.yml` (replace native release publication with explicit manual integration publication).
- `scripts/verify-integration.sh`, `scripts/integration-release.ts` (add verification/publication tooling).
- `docs/releases/integration-publishing.md` (add this current procedure; do not overwrite historical release notes).
- `herdr-plugin.toml` and `scripts/{consumer,install,run-native,materialize-release}.sh` (apply the already prepared runtime consumer candidate as one closure only after real metadata exists).

Keep the final current `native-adapter/{Cargo.toml,Cargo.lock,src/}` and `integrations/{herdr,omp}/` from Herdr; these are not copied from the common app or replaced by this tooling. Preserve current host docs, licensing, source history and reviewed integration-facing documentation.

The existing materializer stages only the manifest/four runtime scripts, **not** this new CI/tooling/package closure. Merge the exact candidate additions above into the final integration tree and copy the materializer's actual `core-release.json` and complete `release-evidence/` alongside them. Track both metadata and evidence in the final reviewed commit. Do not copy the materialized output as if it were a full repository. The retained evidence is release-verification data, never an installation tree or an uploaded integration asset.

At the parent-authorized final cutover, remove/replace these obsolete original owners (this candidate does not delete source originals):

- Remove `.github/workflows/wiki.yml` and `.github/workflows/pages.yml`; replace `.github/workflows/release.yml` as above. There is no new automatic Wiki/Pages/native publisher in this integration closure. Existing public Wiki/Pages history is not deleted or republished by these tools.
- Remove `scripts/{build-rig-native.mjs,package-native.ts,release-docs.ts,release-docs.test.ts,wiki-docs.ts,wiki-docs.test.ts,publish-wiki.sh}`.
- Replace root `package.json`, `bun.lock`, `tsconfig.json`, `.gitignore`; do not retain `build:native`, `test:native`, `package:native`, or `install:local`/`--local-build`. The new `check`, `verify`, `test:adapter`, `release:check`, and `release:publish` commands have integration scope only. Retain only the existing Bun/Node type/compiler dependency closure; Markdown/site/rig dependencies are absent.
- Remove the common `native/`, `web/`, `assets/`, `site/`, `.agents/skills/{character-creator,create-pet-character}/`, `tools/{adapt-rig-source.py,character-pack.py,repair-hair-pockets.mjs,repair-hair-pockets.test.ts,pages.mjs,pages.test.ts}`, and `tools/{native-rig-probe,rig-probe}/` only after the parent's final snapshot/materialized-consumer publication prerequisite. Never remove `native-adapter/`.
- Current generic app docs (`docs/{cli,development,development.ko,usage,usage.ko,usage.ja,usage.zh-Hans}.md`) must be replaced with integration guidance/core links or removed with their retained links repaired by the documentation owner. Keep historical `docs/releases/`, `docs/migrations/`, `docs/validation/` records. Label the old `docs/releases/publishing.md` procedure historical and link this new procedure; do not globally rewrite historical commands/evidence. Historical tag URLs remain the authority for removed old tooling.

## Verification before core publication

The parent can verify the candidate against the retained original integration sources without copying common native code or modifying the original checkout:

```sh
bash migration/integration-cutover/herdr/scripts/verify-integration.sh \
  /absolute/path/to/retained-herdr-checkout
```

This creates a disposable overlay containing only candidate package/config/manifest/scripts plus the retained adapter and integrations. It validates Bash syntax for all consumer/publication shell scripts, Herdr identity/action/attach hooks, the exact contracts revision and lock, then runs `bun install --frozen-lockfile`, the existing `tsc --noEmit` convention against publication tooling and actual OMP TypeScript, and `cargo test --locked --manifest-path native-adapter/Cargo.toml`. It rejects obsolete product owners in the candidate/final root, not the borrowed original source. It needs macOS, Bash, Bun 1.4.2, Rust stable and access to the published dependency revision; it does not install/start/build/package the common app, mutate the original checkout or publish anything. No configured personal Cargo path patches are copied.

In the final integration tree run `bun run verify` (or `bash scripts/verify-integration.sh`) without an argument. `.github/workflows/integration.yml` performs this same gate on pull requests, main pushes and manual dispatch with read-only repository permission. It neither consumes missing release metadata nor publishes tags/releases. This source gate does not prove patched-host attach/runtime discovery/profile behavior or actual native publication; those remain parent-owned runtime gates.

## Real metadata materialization

After the real core v0.4.0 release is public and reviewed, use the existing consumer materializer with the actual anonymously downloaded installer, checksum and reviewed API record:

```sh
bash migration/integration-cutover/herdr/scripts/materialize-release.sh \
  /absolute/desktop-pet-install-v0.4.0.sh \
  /absolute/desktop-pet-install-v0.4.0.sh.sha256 \
  /absolute/reviewed-core-release-api-record.json \
  /absolute/new-materialized-candidate
```

The materializer authenticates the fixed core repository/release, exact approved six asset identities and bytes/checksums and actual resolved tag commit; it emits real metadata/evidence in a new directory. Review the output and apply it with the complete candidate closure. No source/release version or local HEAD is substituted for actual core metadata. A missing metadata file is an explicit blocking error for bootstrap/release, not permission to generate an example.

## Reviewed integration tag and note

The parent prepares a clean complete integration-only commit, completes final verification/runtime acceptance, reviews and publishes the integration tag manually. The publisher never creates/moves tags, never performs a git push and never updates a pre-existing Release.

- `herdr-plugin.toml` stable integration version must select the exact tag, initially `v0.4.0`. It is not compared to adapter crate/root package/native version. The current reviewed consumer metadata explicitly selects actual core v0.4.0 and `integration_api: 1`; later core changes require separately reviewed consumer materialization, not a plugin-version-derived URL.
- Freeze `docs/releases/v0.4.0.md` in that same commit. Its first heading must be `# Desktop Pet — Herdr integration v0.4.0 release notes`. Include the independent `integration_api` contract and actual `https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.0` authority. Use absolute HTTPS links or in-page anchors in the Release body; do not rely on repository-relative Markdown links. This tooling does not write that reviewed note or invent runtime acceptance evidence.
- Supply the complete 40-character reviewed **Herdr** commit SHA. The publisher requires clean HEAD at that SHA and independently resolves the public Herdr tag, including annotated tags, to the same SHA.
- Track reviewed `core-release.json` and complete materialized `release-evidence/`. The publisher verifies the retained record hash, reruns anonymous real core release materialization and compares every stable metadata authority field. Fresh release API download counters/timestamps may change, so it authenticates the frozen evidence hash separately rather than replacing it with a new API-record digest.

Read-only release validation after materialization/tag publication:

```sh
bun scripts/integration-release.ts check v0.4.0 FULL_REVIEWED_HERDR_COMMIT_SHA
```

An optional fourth argument writes the exact reviewed note to a **new** output file; existing files are not overwritten. Without actual metadata, evidence, reviewed note or matching public tag/ref, validation fails. No check automatically publishes.

## Explicitly authorized manual publication

The checked-in Release workflow has only `workflow_dispatch`: no push/tag/release trigger. It checks out the supplied reviewed immutable commit, repeats integration verification and real metadata/tag/note validation, and creates a new **assetless**, non-prerelease integration Release with `--latest=false`. It never uploads a native archive/checksum/rig asset, calls a native packager, invokes Wiki/Pages, or changes bridge/old tags/releases.

Before relying on the workflow, the parent should configure the `integration-release` environment with required reviewers and appropriate deployment restrictions. No repository settings or secrets are changed by this candidate. Dispatch `.github/workflows/release.yml` with exact `tag`, `reviewed_ref`, and authorization `hanbong5938/herdr-desktop-pet@FULL_REVIEWED_HERDR_COMMIT_SHA`. The normal `GITHUB_TOKEN` is exposed only in the publication step; no owner login is copied into secrets.

The equivalent explicitly parent-authorized local command, with a usable scoped `gh` credential already supplied to the process, is:

```sh
HERDR_INTEGRATION_RELEASE_AUTHORIZATION='hanbong5938/herdr-desktop-pet@FULL_REVIEWED_HERDR_COMMIT_SHA' \
  bun scripts/integration-release.ts publish v0.4.0 FULL_REVIEWED_HERDR_COMMIT_SHA
```

`gh release create --verify-tag` refuses an existing release. There is no repair/edit/overwrite mode or native-asset upload argument. Publication then reads the actual Release API and requires the reviewed body and zero assets. Failure remains visible; inspect any real release result manually rather than rerunning native publication or rewriting history.

## Evidence status

This tooling/notes change was implemented without checks, builds, tests, formatters, original-source edits or publication. The parent must exercise candidate verification, real materialization, final source/runtime gates and the separately authorized integration release procedure. Until real core publication supplies metadata and the parent performs the final cutover, the candidate must not replace an installed Herdr consumer.
