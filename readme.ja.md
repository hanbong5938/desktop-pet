# Desktop Pet

独立したネイティブ macOS デスクトップコンパニオンです。ルベリアはタッチに反応し、Herdr・Tern など登録済みプロバイダーのセッションを表示して、許可された送信先に返信します。ホストなしでも動作します。

[English](readme.md) · [한국어](readme.ko.md) · [日本語](readme.ja.md) · [简体中文](readme.zh-Hans.md)

**現在の公開 Core:** [Desktop Pet v0.4.4](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.4)、release `412446874`、公開 `2026-10-14T07:31:12Z`、不変ソース `b7ab7261aa239441273065e1df6ba7402c750454`。匿名公開6アセット・strict/deep署名・一致 protocol 4 / capability 2 / API 1・配備68ファイルの全ハッシュと正確な通常ファイル closure は通過しました。archive SHA-256 `be59d299cdfc631858239e729186c09951da57811635fea3adeab9e21941695c`、installer SHA-256 `4a2b5d222513cae77045229e62f59e2b4d0c871cab0f1904ee1108be00acef33`。[CI 38528323235](https://github.com/hanbong5938/desktop-pet/actions/runs/38528323235) の release job `115733881402` は通過しました。後続 Wiki job `115735603636` は実際に Linux で **管理ページ38個を描画**した後、`WIKI_PUBLISH_TOKEN is missing; use a dedicated credential with Wiki write access` で失敗しました。Wiki 公開には所有者による Wiki Home 初期化と専用 Wiki-write secret が必要です。広範な所有者トークンを再利用せず、Wiki 修復のためにバイナリ Release を再実行しません。最終 current Pages の配備と4言語の描画は未検証です。

中立 v0.4.4 formula は tap `2aa9a4be9517e0232a3f2b8e2b3c748045c69586` に公開済みです。適格な Brew v0.4.4 新規導入は通過しました（74ファイル / 44.3 MB、内外の再署名5個）。認証済み検査で Homebrew 派生 image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312` を確認しました。導入・検査は中立 config/state・standalone app を作成せず、デーモンも起動しませんでした。明示的な手動 **Start** の後だけ実際の v0.4.4 UI/control ready、PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470` に達しました。ルベリアと実際の Settings は **Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0** を表示し、その後の明示 Hidden は同じ既定 instance を保持しました。新規導入の証拠であり、v0.4.4 通常更新や managed 移行の完了証明ではありません。

**現在の公開 bridge:** [Herdr bridge v0.3.9](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.9)、release `412458870`、公開 `2026-10-14T07:47:14Z`、ソース `0f33f2806412fe814367f58bdfbbe2e833399a3e`、tree `1572984289c3eb2049b4d5b17c27b013c2ca593a`。正確な **通常 UTF-8 Git ファイル11個**は metadata/downloader/license/readme と狭い `/dist/HerdrDesktopPet.app/` ignore のみを含みます。producer scripts/manifest/ignore の6 blob は公開 Core `b7ab7261aa239441273065e1df6ba7402c750454` と一致し、retirement は Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8` を維持します。匿名公開5アセットは独立検証 CI bytes・strict署名・一致 protocol 3 / capability 1 / API 1・正確な68ファイル hash closure と一致します。archive SHA-256 `061dc6cb04e1512db90f0ffe1e9e261fd2be0935924e189ca0c7f8ce3c43808c`。[Bridge CI 38529018457](https://github.com/hanbong5938/desktop-pet/actions/runs/38529018457)、job `115736118653` は実際の shipped-native `TMPDIR=/tmp` と実 Git admission を含め通過しました。生成アプリは clean、tracked 変更・無関係な root ファイル・`dist/user-owned-note` は dirty のままです。old stable formula tap `349e3074ef41960096fbfad884f0ef4b17930e0f` は v0.3.9 を選択します。凍結 v0.3.6 は歴史的 latest、beta formula・タグ・アセットは不変です。

**実際の最終公開 managed A 完了:** 凍結 v0.3.6 Check は公開 v0.3.9 を選択し、`preferences/last_update_check` のみ変更しました（`1791963743` → `1791965310`）。Check 中は他の4 tree bytes・source PID `2467`・無関係な既定 PID `2641` が不変です。正確な source `0f33f28` の native A 同意で operation `d5b2bf6f02e031eabb5a8c8fdd805cab` が完了しました（`completed` / `manager_exited`、installed/applied true）。実際の公開 v0.3.9 native PID `3417` / instance `b42b25d48315e66bd87e7685e58cfb14`、image `0ed12d159b6bbb83ada833d39b4524ac5f7eccb1253b7acaf9aa2b71cbf833ce` は正確な旧 private config/state で一致 protocol 3 / capability 1 / API 1・UI/control ready・Herdr 1 / Tern 0 に達しました。実 managed source の Git status は完全に clean です。保持した元の v0.3.6 helper の strict/deep署名と実際の completed status も通過しました。baseline は旧 A 証拠87個と全 state ファイル91個を hash で保存します。

**実際の最終公開 managed B 完了:** 別の Move preview と独立 strict/deep 検証済み公開 v0.4.4 coordinator が native **Migrate** 同意を受け、operation `d729fd740f4e459945edda00bf48d944`、plan SHA-256 `6471e928254229a66a6fc64a88f67095777d6bd890d621feb36b08a4c7d1fc69` で **COMPLETE** に達しました。Native Prepare/Stop で公開 v0.3.9 PID `3417` / instance `b42b25d48315e66bd87e7685e58cfb14` を停止し、適格な既存 Brew v0.4.4 app root/image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312` を使用しました。`target_manager:null` のため **再導入はありません**。Target PID `4104` / instance `8912b3aad5419b1c14a78ef3418ae36f` は正確な旧 private config/state と保存 Herdr 選択（Herdr 1 / Tern 0）を維持し、protocol 4 / capability 2 / API 1・実 UI/control ready に達しました。native_ready・target_committed は true です。実 Herdr retirement attempt 1 は PID = PGID `4140`、exit 0、quiescent true、error null です。実 plugin list は不変 Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8` の enabled integration-only Desktop Pet を表示し、managed Git HEAD も一致しました。実 manager 出力は v0.4.1・21 actions・startup/client-attached hooks です。Managed dist/app と旧 PID `3417` は消え、standalone `DesktopPet.app` は作成しませんでした。正確な retired v0.4.1 タグの歴史的 Core v0.4.2-minimum wrapper は同じ v0.4.4 PID/instance を実際に再利用しました。新 app・新 tag はありません。元の A 証拠87個の SHA-256 と既存旧 source-config hash は全て不変、無関係な既定 config/state の両 tree は byte-identical です。hidden の既定 v0.4.4 PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470` は不変で稼働中です。Private XDG base profile を adopt し、global default と merge していません。実 GUI は Desktop Pet / 内蔵ルベリア・Settings **Herdr Connected 1 / Tern Disabled 0** と視覚確認し、clean-image capture のみ保留です。Fault のない最終公開 v0.3.9/v0.4.4 A/B happy path は、別途通過した過去 working metadata `f7edf5e` の definite failure/明示 Resume 証明と区別します。**実 beta ingress・中立 v0.4.4 通常更新・実 SDK 両起動順序/prompt/transcript/reconnect・最終 current Pages は引き続き保留で、全 matrix は未完了です。**

**consumer・SDK 境界:** Herdr 連携は公開 v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`（assetless release `412252863`）を維持します。歴史的 Core v0.4.2 minimum は凍結され、健全な Core v0.4.3 再利用は観測済みです。現在の Herdr consumer scripts と metadata/evidence 全8ファイルは、公開 Core v0.4.4 release `412446874` / ソース `b7ab7261aa239441273065e1df6ba7402c750454` から実際に materialize されました。新 Herdr タグはなく、最終公開 managed retirement と不変 wrapper の v0.4.4 再利用は上記の実 B 結果で別途確認しました。Tern v0.2.0 と互換 Core v0.4.1 pin は不変です。Tern は検証済み実行ファイルと実際の config/state 組を明示 **Configure → Save → Connect** で接続し、アプリを導入・自動起動しません。実 Tern keychain/provider sign-in と genuine OMP 18.10.3 providerSignIn/NoModel は両起動順序の同一 instance/prompt/transcript/reconnect 受入れを引き続き阻止しています。資格情報の複製・偽アカウント・ACK・完了で代替しません。**全 matrix は完了していません。**

**過去の v0.4.2 / v0.3.7 根拠（v0.4.4 実行証明ではない）:** **公開状態:** [Desktop Pet v0.4.2](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.2) は公開済みです（release `412241744`、`2026-10-14T01:09:50Z`、不変ソース `cb10ce2c3cc65402cbb914d60301f7627d5546d5`）。匿名6アセット、strict/deep署名、protocol 4 / capability 2 / integration API 1、配備68リソースの全ハッシュと正確な closure を検証しました。tap `32919c745864b54a2e716fcc5a3714d0d38d3e84` の中立 formula と実際の公開 v0.4.1 → v0.4.2 Brew 更新で receipt/derivation、署名5個、`brew test`、Auto のネイティブ UI/control ready（Herdr 0 / Tern 0）を確認しました。新規公開 standalone v0.4.1 導入と同時インストーラー2個による稼働中所有者の再利用を確認し、ネイティブ Check が実際の公開 v0.4.2 を提示、同意後 `standalone_replace`、`manager_exited`、`installed:true`、`applied:true` に到達しました。同じ config/state と正確な公開 v0.4.2 image で準備完了です。Check は locator/profile/lifecycle/locks/PID を保持しましたが `preferences.json` の確認キャッシュは変化したため、全設定バイト不変ではありません。[修正 integration-only Herdr v0.4.1](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.4.1) も公開済みです（release `412252863`、不変 ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`）。公開 wrapper 2個は非表示のまま同じ Brew v0.4.2 PID/instance/image を再利用し、明示 config/state と実際の private patched Herdr 0.9.3 socket で Auto Herdr 1 / Tern 0 に到達しました。新規環境の公開 bootstrap 2個は OS-home のアプリ1個を導入/再利用し、連携内 native ファイルやデーモン起動はありません。これは公開 Herdr manager 導入 receipt の証拠ではありません。元の Herdr v0.4.0 の生成アプリ/失敗テスト scaffold symlink ソース欠陥は保持し、integration-only だったとは主張しません。[Bridge v0.3.7](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.7) も公開済みです（release `412267469`、不変 tag `987a4b859f95e922c1c5443ead372d10d0dddb24`）。正確な Core `cb10ce2` と修正 Herdr `6d2ac04` から構築し、匿名5アセット・署名・68ファイル closure を検証しました。tap `657f32cc4cc563ae40ae0432ac5123dcb3f58426` で old stable formula のみ v0.3.7 に更新、beta は不変です。manual-origin の private 実ホスト実行は Herdr 1 / Tern 0 ready で、managed A→B の証拠ではありません。実際の凍結公開 Brew v0.3.6 → v0.3.7 A `6a10663c2d2f0177990623a969238af4` と別途同意した Core v0.4.2 B `c66961f9e5a91c0023545065ae61385b` は、実際の Brew retirement（exit 0、quiescent）まで完了しました。B 前後で全設定ファイルのハッシュと旧 A 証拠87ファイルは不変で、実際の Core v0.4.2 UI は Desktop Pet / Herdr Waiting 0 / Tern Disabled 0 です。一方、実際の managed v0.3.7 A `a1aec5902020de7bcaaf583b1a5f50a5` は native `/tmp` プロファイル検証に失敗しました。checkout 置換・registry 登録前に manager exit 1 となり、凍結 helper は Unknown / `manager_exited`、installed/applied false を保持しています。VM・journal・reservation は recovery/replay なしで保存されています。

**過去の公開 v0.3.8 managed 結果・安全な successor 境界:** managed v0.3.6 → 公開 v0.3.8 A は完了し（`16b6307413ec409efe3eb5a04c9cc21f`）、元の署名済み v0.3.6 helper が completed status を確認しました。明示 Check 後の B preflight は `managed source changed; migration never resets user changes` で拒否しました。`dist/HerdrDesktopPet.app` の生成ファイルちょうど68個が untracked でした。B 同意・Stop 前のため B manager・fault flag はなく、当時の source PID `3563` は不変でした。元の公開 v0.3.8 タグは通常 UTF-8 Git ファイル10個で `.gitignore` はありません。B 制限は歴史的事実で、公開 v0.3.9 が古いタグを遡って修正するものではありません。手動 ignore・`git clean`・reset で旧タグを通してはいけません。managed v0.3.8 A 完了済みの利用者は、保持した **一致する署名 helper** で既知の terminal 旧 operation を確認し、実 config/state・旧証拠をバックアップし、source を明示 **Stop** してから **実際の所有 Herdr manager** で successor を明示導入します。Unknown manager operation は絶対 replay せず、VM・journal・reservation を保持します。

**過去の working-metadata・通常更新証明（最終 v0.3.9/v0.4.4 受入れではない）:** genuine managed B は working metadata `f7edf5e0eba374248b9a5ee9d66d26109b39eca2`・不変の公開 v0.3.8 native・公開 Core v0.4.3 で NativeReady/Commit 後に definite retirement failure（manager exit 1、quiescent）となりました。導入した所有 `uchg` flag のみを除去し、同じ plan SHA `3615fa140fd1b0d49910f5f2ef170b64122243c6c65d0098d4ecfd729e951f16` に独立の明示 Resume 同意を得て attempt 2 が完了しました（実 CLI exit 0、quiescent）。旧 managed app/source は retirement され、Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8` が同じ v0.4.3 instance を再利用しました。旧 A 証拠87個・first-attempt/plan/consent 5個・旧 config・無関係な既定 profile の hash は不変です。実際の公開中立 v0.4.2 → v0.4.3 通常更新も完了しました（`81c82d837b65fb03584bfb1e668119c3`、`standalone_replace`）。同じ profile/Herdr 0 ready・ルベリア UI を確認し、Herdr 再導入はありません。Check は `last_update_check` のみ変更し、Check 後は全 config bytes と旧 A 証拠87個が更新を通じ不変でした。最終公開 v0.3.9 A/B や v0.4.4 更新の証明ではありません。


<img src="assets/rubelia-thumbnail.png" alt="標準デスクトップコンパニオンのルベリア" width="220">

## 機能

- 移動・サイズ変更可能なデスクトップキャラクター。タッチや撫でる操作への反応、独立した状態バブル、メニューバー操作に対応。UI はシステム言語、韓国語、英語、簡体字中国語、日本語から選択できます。
- 観測した Herdr セッションのカード。検索・絞り込み・並べ替えと、ローカル／リモートの明確な区別。リモート観測は任意で読み取り専用です。
- 選択した**ローカル**エージェントセッションへの一行返信。IME の変換中入力に配慮し、ネイティブの状態フィードバックを表示します。
- キャラクター、バブル、セリフ、表示状態の設定。検証済み追加パックと公式カタログブラウザーに対応します。
- Herdr のライフサイクル操作、保護付きの linked worktree 削除、インストール元を区別した明示的なアプリ更新。
- **capability 2：**統合された接続設定で Auto または登録されたプロバイダー（Herdr／Tern など）を選び、実際の接続状態を確認し、共有プロファイルへ稼働中の変更を保存します。現在の公開 Desktop Pet v0.4.4 はこの UI を含みますが、過去の Herdr v0.3.6 にはありません。2ホストの実アカウント／プロバイダー実行検証完了を意味しません。

## 動作要件

Apple Silicon Mac、macOS 13 以降。ソース構築には Rust/Cargo、Bun、Node.js/npm、Xcode Command Line Tools が必要ですが、プリビルドには不要です。ad-hoc 署名で、公証されていません。Herdr スナップショットは互換ローカルサーバー（0.9.0+）、返信は別途 `agent.prompt` が必要です。任意の client-attach 自動起動には公式 Herdr 0.9.3 に[連携所有のパッチとホスト手順](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)を適用するか、実際にフック対応のホストが必要です。標準 0.9.0/0.9.3 にはフックがなく、プラグイン導入はホストをパッチしません。

## インストールと任意の連携

中立 v0.4.4 formula は tap `2aa9a4be9517e0232a3f2b8e2b3c748045c69586` に公開済みです。適格な Brew v0.4.4 新規導入は通過しました（74ファイル / 44.3 MB、内外の再署名5個）。認証済み検査で Homebrew 派生 image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312` を確認しました。導入・検査は中立 config/state・standalone app を作成せず、デーモンも起動しませんでした。明示的な手動 **Start** の後だけ実際の v0.4.4 UI/control ready、PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470` に達しました。ルベリアと実際の Settings は **Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0** を表示し、その後の明示 Hidden は同じ既定 instance を保持しました。新規導入の証拠であり、v0.4.4 通常更新や managed 移行の完了証明ではありません。導入コマンド:

```sh
brew install hanbong5938/tap/desktop-pet
desktop-pet --version
desktop-pet start
desktop-pet status
```

プリビルド・ソース導入は Herdr 連携ではなく **core** チェックアウトで実行します。既定はプリビルドのみで、取得・検証失敗からソースビルドへ黙って切り替えません。`--source` は現在の core だけを構築し、その `dist/DesktopPet.app` を使用します。

```sh
git clone https://github.com/hanbong5938/desktop-pet.git
cd desktop-pet
# 公開済みの不変 v0.4.4 インストーラーと固定アセットを使用:
git checkout --detach v0.4.4
bash scripts/install.sh --prebuilt
# 同じ公開 v0.4.4 タグでソース導入を選ぶ場合:
# bash scripts/install.sh --source
# Replace with the installer-reported executable and your actual profile paths:
/absolute/reported/desktop-pet --config-dir /absolute/dev/config --state-dir /absolute/dev/state start
```

不変 v0.4.4 の既定／`--prebuilt` は正確な公開固定アセットを使用します（過去の v0.4.0／v0.4.1 は各 pin を保持）。取得／検証失敗をソース構築で隠しません。明示 `--source` は実際の公開 v0.4.4 タグを含む選択 core checkout を構築します。 standalone prebuilt は **実 OS アカウント home** の `Applications/DesktopPet.app`、承認された source は core の `dist/DesktopPet.app` を使います。admission は検証済み健全な既存所有者を再利用し得るため、推測した dist パスではなく導入器が示す実行ファイルを使います。共有 locator はアカウント home の `.local/share/desktop-pet/installation.json` に原子的に保存し、探索証拠であって権限ではありません。不正な明示パス／locator は fallback せず、実行ファイル、署名、capability、origin を再検証します。Brew は実際の formula／prefix を所有し、生存／予約中の導入を密かに引き継ぎません。

必要な連携だけを導入します。Herdr は薄い consumer です。Tern はネイティブアプリを取得・構築・インストール・自動起動しません。プラグインの更新・削除は共有アプリ、プロファイル、キャラクターを削除しません。通常のアプリ更新で Herdr プラグインを再導入しません。

```sh
herdr plugin install hanbong5938/herdr-desktop-pet
tern plugin install https://github.com/hanbong5938/tern-desktop-pet
```

## プロファイル・接続・移行

`--host auto|PROVIDER` は Auto または登録された不透明な提供者の選択で、固定 Herdr/Tern 列挙ではありません。観測選択の優先順位は CLI > `DESKTOP_PET_OBSERVATION_MODE` > 保存 `observation.host` > 新規 Auto です。旧 `both`→`auto`、host-less→Herdr は読み込み移行だけに適用します。実行中は Connection／`preferences set --observation-host auto|PROVIDER` で保存後に適用します。`--herdr-socket ABS` は登録 Herdr アダプターの backend オプションで core 制御 socket ではなく、アダプターなしでは登録しません。

新規プロファイルは Auto です。Connection は登録されたプロバイダーの実際の接続を示し、インストール有無を接続と扱いません。ID は不透明な値で、権限は名前の接頭辞やパスではなく capability で判断します。Auto は同時観測で、fallback ではありません。リモートは読み取り専用、Tern は返信のみで worktree 削除は不可です。受付はエージェントの完了ではありません。

[利用ガイド](docs/usage.ja.md)、[正確なプロファイル・探索契約](docs/development.md#profiles-and-installation-discovery)、[A→B 移行](docs/migrations/desktop-pet.md)を参照してください。status / Connection の実際の実行ファイル・config・state を Tern Configure に指定し Save の成功後 Connect します。既存の Tern・beta・custom プロファイルはその場で保持し、マージしません。

```sh
desktop-pet profile show --json
desktop-pet start
# Only with the registered Herdr adapter; use its verified backend:
# desktop-pet --herdr-socket /actual/path/to/herdr-api.sock start
desktop-pet preferences set --observation-host auto
```

## 操作

| 操作 | 結果 |
| --- | --- |
| 頭／体をタップ、頭の上をなぞる | ルベリアが反応し、撫でられます。 |
| キャラクターをドラッグ、右下のグリップを使う | 移動、サイズ変更します。 |
| バブルの背景または辺／角をドラッグ | 付属キャラクターまたは独立したバブルを移動し、バブルを個別にサイズ変更します。 |
| メニューバーのアイコンを左クリック | キャラクター、観測、バブル、ライフサイクルの設定を開きます。 |
| バブルを開き、ローカルエージェントカードを選択 | 一行返信を開きます。Enter、Command+Enter、送信ボタンで送信します。リモートカードは読み取り専用です。 |

IME 変換中は**決して送信されません**。Escape はまず変換を取り消します。検索欄で Enter／Command+Enter を押しても返信は送信されません。キャラクターとバブルの表示は独立しており、両方を隠すかウィンドウ全体をクリック透過にすると、デスクトップ上のクリックで操作できなくなる場合があります。その際はメニューバーのアイコンから**キャラクターを表示して有効化**する復旧操作を使用します（標準モードでは常にアイコンを表示し、復旧時のみのモードでは必要なときに表示）。すべての操作、検索、保存済みマシン経由のリモート観測、表示動作は[利用ガイド](docs/usage.ja.md#操作)を参照してください。

## 追加キャラクターと衣装

[キャラクターギャラリー](https://hanbong5938.github.io/herdr-characters/)では追加パックをプレビューでき、[別リポジトリ](https://github.com/hanbong5938/herdr-characters)にソースと利用条件があります。設定の**キャラクター → キャラクターを探す…**を選んでください。肖像画を見るだけではアーカイブはダウンロードされません。対象となる公開済み公式パックには明示的で検証済みの**ダウンロードして適用**操作があります。ローカルフォルダーや `.herdrchar` の場合、**読み込み**だけでは有効化されません。インストール済みカード／リビジョンを選び、**適用**をクリックしてください。同梱・標準キャラクターはルベリアだけです。[キャラクターパック](docs/usage.ja.md#キャラクターパック)と[制作方法（英語）](docs/development.md#character-authoring)も参照してください。

## 安全上の注意と制限

- 返信には選択元の**ローカル prompt capability**が必要です。Herdr は `agent.prompt`、Tern は１回限りの `agents:ask` を使い、遠隔、SSH、shell、PTY 入力に迂回しません。受付 ACK は**モデルの完了ではなく**、Tern は queued の場合があります。承認／質問は実際のターミナルで扱い、不確実な配送は確認なく再送しません。Herdr の提出 API には現在 occupant の競合があり、セッションの原子的固定を保証しません。[配送詳細](docs/usage.ja.md#インライン返信とメッセージ配送)。
- **Worktree を削除…**は元に戻せません。続行前に**正確なチェックアウトパス**と**ワークスペース全体**のタブ／ペインを確認してください。そこにあるプロセス／エージェントを終了し、**無視されているファイルも含めて**リンク済みチェックアウトを削除します。ゴミ箱への移動や取り消しはできません。メインのリポジトリとブランチは削除しません。[削除時の安全策](docs/usage.ja.md#ワークツリーの削除)。
- リモート観測には、選択済みの保存されたマシン設定と互換性のあるリモート Herdr サーバーが必要ですが、**リモート側へのキャラクターのインストールは不要**です。ローカルアプリはリモートの状態をポーリングするため、見逃した短い状態遷移を復元したり、リモートのプロンプトを送ったりはできません。[観測ガイド](docs/usage.ja.md#観測元)。
- アプリ更新の自動確認は lifecycle・導入・image・選択 profile を保持しますが、`preferences.json` の成功確認 timestamp キャッシュを書き込めます。更新のインストール／適用には明示的な同意が必要です。更新機能の所有権 ACK は**完了を意味しません**。結果が不明な場合は再試行前に[アプリ内更新の復旧](docs/usage.ja.md#アプリ内更新)と [CLI 更新状態と復旧（英語）](docs/cli.md#app-update-status-and-recovery)を参照してください。

## 設定とトラブルシューティング

プロファイル優先順位は **明示的 `--config-dir`/`--state-dir` > `DESKTOP_PET_CONFIG_DIR`/`DESKTOP_PET_STATE_DIR` > default-profile record > 中立 XDG 既定値**（未指定なら `~/.config/desktop-pet`、`~/.local/state/desktop-pet`）です。core は `HERDR_PLUGIN_*` を暗黙使用せず、Herdr wrapper だけがホスト注入を明示引数に変換します。config は lifecycle/preferences、characters、menu-bar-icons、state は lock、私有 `control.sock`、daemon log、更新証拠を保持します。実際の両パスを合わせてバックアップします。`--herdr-socket ABS` は登録 Herdr アダプターの backend オプションで制御 socket ではなく、アダプターなしで core は正規化／登録しません。[正確なプロファイル・探索契約](development.md#profiles-and-installation-discovery)。

Core は共通 lifecycle 保存と `auto_start`（既定 on）を所有し、追加キーは登録連携が所有します。Herdr 所有者は **`lifecycle.json` が存在しない場合だけ**、最初の lifecycle 書込みで共有 profile writer 内に `exit_with_herdr:true` を原子的に初期化します。既存ファイルにキーがなければ false、明示的 false も維持します。旧真偽値 `enabled` は false を含め `auto_start` に移行します。端点、起動メタデータ、他所有者のフィールドは保持し、`preferences.json` ではなく `lifecycle.json` に保存します。不正 JSON／値は既定化せずエラー、保存失敗は既存ポリシーを維持します。Auto／アダプターなしは Herdr がなくても動作を維持し、以下の Herdr 端点／切断規則は Herdr 専用選択だけに適用します。

## 資料とウェブサイト

[Desktop Pet Wiki](https://github.com/hanbong5938/desktop-pet/wiki) は core 所有の生成された閲覧文書で、レビュー済みソースが原本です。実際の Wiki 公開は別途確認します。旧 Herdr サイトは第二のアプリサイトではなく連携・移行の入口です。

- [使い方と復旧](docs/usage.ja.md) · [開発とインストール（英語）](docs/development.md) · [ネイティブ自動化 CLI 契約（英語）](docs/cli.md#english) · [パッチ済みホスト連携（英語）](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)。
- [リリース履歴（英語）](docs/releases/README.md) · [v0.3.6 の変更点（英語／韓国語）](docs/releases/v0.3.6.md) · [v0.3 更新ガイド（英語）](docs/releases/0.3.md#upgrade-from-v021-or-beta) · [過去のベータ／プレビュー案内（英語）](docs/migrations/unreleased.md) · [v0.1.11 → v0.2.0 移行（英語）](docs/migrations/v0.1.11-to-v0.2.0.md)。
- 宣伝用の[日本語](https://hanbong5938.github.io/desktop-pet/ja/)・[韓国語](https://hanbong5938.github.io/desktop-pet/)・[英語](https://hanbong5938.github.io/desktop-pet/en/)・[簡体字中国語](https://hanbong5938.github.io/desktop-pet/zh-Hans/)ウェブサイトはこの**ネイティブ**アプリを紹介しています。ブラウザー内でアプリを動かすものではありません。中立 Pages の公開はここでは未確認です。ウェブサイトの開発／公開は [GitHub Pages ガイド（英語）](docs/development.md#website-github-pages)を参照してください。別の[キャラクターギャラリー](https://hanbong5938.github.io/herdr-characters/)では追加パックを紹介しており、対応言語は英語と韓国語のみです。

## アートとライセンス

[ソフトウェアライセンス](LICENSE.txt)とアート／モデルの利用条件は**別物**です。MIT ソフトウェアライセンスによってキャラクターのアートが再ライセンスされることはありません。同梱ルベリアの[ライセンス](assets/rubelia-default/LICENSE.txt)、[帰属表示](assets/rubelia-default/ATTRIBUTION.txt)、[出典記録](assets/rubelia-default/source-record.json)、[背景修復記録](assets/rubelia-default/background-repair.json)が正式な情報です。`LICENSE.txt` には元の Coding Cat の告知も残っています。そのアート／ソースは[キャラクターリポジトリ](https://github.com/hanbong5938/herdr-characters)へ移されました。各追加パック独自の条件を必ず保持してください。アート所有者の承認はモデルの権利を与えません。Qwen の研究用ライセンスは制作記録であり、モデルのエンジンや重みは同梱されていません。[キャラクター制作とライセンス（英語）](docs/development.md#character-authoring)を参照してください。
