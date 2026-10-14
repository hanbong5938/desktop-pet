# Desktop Pet

独立したネイティブ macOS デスクトップコンパニオンです。ルベリアはタッチに反応し、Herdr・Tern など登録済みプロバイダーのセッションを表示して、許可された送信先に返信します。ホストなしでも動作します。

[English](readme.md) · [한국어](readme.ko.md) · [日本語](readme.ja.md) · [简体中文](readme.zh-Hans.md)

**現在の公開 Core:** [Desktop Pet v0.4.3](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.3)、release `412334201`、公開 `2026-10-14T04:20:48Z`、不変ソース `9bba2e71a1d5a1bcef5f71509ac6701b61b3d674`。匿名公開6アセット・strict/deep署名・一致 protocol 4 / capability 2 / API 1・配備68ファイルの全ハッシュと正確な通常ファイル closure を検証しました。archive SHA-256 `b1bdf3b118876f5b705617b30dc321dd9cb1ff71cff147bcd19314e072ea746a`、installer SHA-256 `7d5094f89a7433759ac102f5028217f057d3bbbfa70a0a6f686bbc5a7559f576`。[CI 38510441950](https://github.com/hanbong5938/desktop-pet/actions/runs/38510441950) の release job `115677463431` は成功し、後続 Wiki job のみ失敗しました。実際の37ページ描画は通過しましたが専用 `WIKI_PUBLISH_TOKEN` がなく未公開です。広範な所有者トークンを再利用しません。中立 v0.4.3 formula は tap `6a9ae670cb3c69eb6e66bfd99ef351c513168f72` に公開済みです。使い捨て VM の適格 Brew v0.4.3 新規導入が通過し、実際のネイティブ GUI は Auto / Herdr Waiting 0 / Tern Waiting 0、PID `2967`、instance `d62441eca0603028793fa846aa0555cd`、派生 image `7b3613635d8cb5e6c26e2b4f4259aa4eeb87bf8eb17c21c7219236e92631c4ae` を示しました。明示 hide は同じ別の既定プロファイルを維持しました。[Bridge v0.3.8](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.8) は公開済みです。release `412343376`、公開 `2026-10-14T04:40:30Z`、通常 UTF-8 Git ファイル10個だけの metadata-only ソース `e615ed310202d99d4389fb80d7cfc29db87275c6`、retirement ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`。匿名公開5アセットは検証済み CI バイトと一致し、archive SHA-256 `fc1d69dab9e9287dcb9903bc262b1b2e0ba06690c1b944230b81439b4ff09003`、rig SHA-256 `54ea2e40171407e243726697e6a56eff226244a32f33318ab5d45fd2c8fdda9b`、一致 protocol 3 / observation 1 / API 1・署名・68個の closure ハッシュを検証しました。CI `38511259163` / job `115680064263` は実際の出荷 ingress の native Rubelia 検証を `TMPDIR=/tmp` で通過し、両 ingress scratch ディレクトリは `/private/tmp` です。old stable formula `49fa8a2` は v0.3.8、beta formula と凍結 v0.3.6 アセット/タグは不変です。新しい実際の managed v0.3.6 → 公開 v0.3.8 A は完了しました（`16b6307413ec409efe3eb5a04c9cc21f`、`completed` / `manager_exited`、installed/applied true）。元の署名済み v0.3.6 helper が確認し、bridge PID `3563` / instance `9aa35111b6b8e54dfca787b01e3425d7` は正確な image `be4858803ceec46a9d3ac80cefb2c7d2da0013f1e2e9df49ca99fca01ae675a8`、同じ旧 private config/state、Herdr mode / Herdr 1 / Tern 0、UI/control ready に達しました。公開 managed A の `/tmp` 修正の証明であり、保存された失敗 VM の recovery ではありません。Core v0.4.3 managed B・明示 failed-retirement resume・実際の v0.3.8 固定 v0.3.6 beta 経路・中立 v0.4.2 → v0.4.3 更新は保留です。公開 Herdr v0.4.1 の互換 Core v0.4.2 pin は保持し、current-main Core v0.4.3 消費者作業は新しい Herdr リリースではありません。**過去の v0.4.2 / v0.3.7 根拠（v0.4.3 実行証明ではない）:** **公開状態:** [Desktop Pet v0.4.2](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.2) は公開済みです（release `412241744`、`2026-10-14T01:09:50Z`、不変ソース `cb10ce2c3cc65402cbb914d60301f7627d5546d5`）。匿名6アセット、strict/deep署名、protocol 4 / capability 2 / integration API 1、配備68リソースの全ハッシュと正確な closure を検証しました。tap `32919c745864b54a2e716fcc5a3714d0d38d3e84` の中立 formula と実際の公開 v0.4.1 → v0.4.2 Brew 更新で receipt/derivation、署名5個、`brew test`、Auto のネイティブ UI/control ready（Herdr 0 / Tern 0）を確認しました。新規公開 standalone v0.4.1 導入と同時インストーラー2個による稼働中所有者の再利用を確認し、ネイティブ Check が実際の公開 v0.4.2 を提示、同意後 `standalone_replace`、`manager_exited`、`installed:true`、`applied:true` に到達しました。同じ config/state と正確な公開 v0.4.2 image で準備完了です。Check は locator/profile/lifecycle/locks/PID を保持しましたが `preferences.json` の確認キャッシュは変化したため、全設定バイト不変ではありません。[修正 integration-only Herdr v0.4.1](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.4.1) も公開済みです（release `412252863`、不変 ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`）。公開 wrapper 2個は非表示のまま同じ Brew v0.4.2 PID/instance/image を再利用し、明示 config/state と実際の private patched Herdr 0.9.3 socket で Auto Herdr 1 / Tern 0 に到達しました。新規環境の公開 bootstrap 2個は OS-home のアプリ1個を導入/再利用し、連携内 native ファイルやデーモン起動はありません。これは公開 Herdr manager 導入 receipt の証拠ではありません。元の Herdr v0.4.0 の生成アプリ/失敗テスト scaffold symlink ソース欠陥は保持し、integration-only だったとは主張しません。[Bridge v0.3.7](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.7) も公開済みです（release `412267469`、不変 tag `987a4b859f95e922c1c5443ead372d10d0dddb24`）。正確な Core `cb10ce2` と修正 Herdr `6d2ac04` から構築し、匿名5アセット・署名・68ファイル closure を検証しました。tap `657f32cc4cc563ae40ae0432ac5123dcb3f58426` で old stable formula のみ v0.3.7 に更新、beta は不変です。manual-origin の private 実ホスト実行は Herdr 1 / Tern 0 ready で、managed A→B の証拠ではありません。実際の凍結公開 Brew v0.3.6 → v0.3.7 A `6a10663c2d2f0177990623a969238af4` と別途同意した Core v0.4.2 B `c66961f9e5a91c0023545065ae61385b` は、実際の Brew retirement（exit 0、quiescent）まで完了しました。B 前後で全設定ファイルのハッシュと旧 A 証拠87ファイルは不変で、実際の Core v0.4.2 UI は Desktop Pet / Herdr Waiting 0 / Tern Disabled 0 です。一方、実際の managed v0.3.7 A `a1aec5902020de7bcaaf583b1a5f50a5` は native `/tmp` プロファイル検証に失敗しました。checkout 置換・registry 登録前に manager exit 1 となり、凍結 helper は Unknown / `manager_exited`、installed/applied false を保持しています。VM・journal・reservation は recovery/replay なしで保存されています。全 matrix 完了ではありません。明示 definite-failed-retirement resume、実 SDK の両起動順序での同一 instance/再接続、アカウント受入れは未証明です。Core v0.4.3・bridge v0.3.8 は公開済みで、Brew v0.4.3 新規導入・新 managed A は完了しましたが managed B/resume 受入れは保留です。 [Core v0.4.3 / bridge v0.3.8](https://github.com/hanbong5938/desktop-pet/blob/main/docs/releases/v0.4.3.md). Tern v0.2.0 と互換 Core v0.4.1 pin は不変で、実際の Tern keychain/provider ログインと OMP providerSignIn/NoModel の解決が必要です。過去のタグ・本文・アセット・ソースは保持します。Core Pages `cb10ce2` 配備と EN/KO ブラウザ確認は成功しましたが JA/ZH 視覚確認と最終現行文書の配備は保留中です。Linux Wiki catalog render は成功しましたが専用 `WIKI_PUBLISH_TOKEN` と所有者による Wiki Home 初期化が必要です。Wiki リンクは公開の証拠ではなく、バイナリ release job 成功と後続 Wiki 失敗は別です。

**公開 managed B の既知の制限・修正待ち:** 実際の managed 0.3.6 → 公開 bridge 0.3.8 A は完了し（`16b6307413ec409efe3eb5a04c9cc21f`）、元の 0.3.6 helper の署名と completed status も検証済みです。明示 Check が A の旧 terminal 観測を整理し Move が有効になりましたが、B preflight は `managed source changed; migration never resets user changes` で拒否されました。実際の Git status は `dist/HerdrDesktopPet.app` 配下の生成ファイルちょうど68個だけが untracked で、他の変更はありませんでした。B 同意・Stop より前で、B manager・fault flag はなく、元の PID 3563 は変更なく稼働中です。公開 Core 0.4.3 / bridge 0.3.8 が現在の版です。承認された新 Core 0.4.4 + genuine bridge 0.3.9 は保留です。producer 修正は `.gitignore` に `/dist/HerdrDesktopPet.app/` だけを宣言し、実際の materialized Git admission gate で生成アプリは clean、tracked script の変更・無関係な root ファイル・`dist/user-owned-note` は dirty のままと確認します。native Git dirty 保護は変更しません。公開 0.3.8 を通すための `git clean`・reset・手動 ignore は行わないでください。元の 0.3.8 タグは `.gitignore` なしの通常 UTF-8 Git ファイル10個、修正された新 metadata は11個です。新公開・managed B/resume・最終4言語 Pages 完了は主張しません。過去の 0.3.7 Unknown/no-replay と SDK/Wiki の障害は維持します。


<img src="assets/rubelia-thumbnail.png" alt="標準デスクトップコンパニオンのルベリア" width="220">

## 機能

- 移動・サイズ変更可能なデスクトップキャラクター。タッチや撫でる操作への反応、独立した状態バブル、メニューバー操作に対応。UI はシステム言語、韓国語、英語、簡体字中国語、日本語から選択できます。
- 観測した Herdr セッションのカード。検索・絞り込み・並べ替えと、ローカル／リモートの明確な区別。リモート観測は任意で読み取り専用です。
- 選択した**ローカル**エージェントセッションへの一行返信。IME の変換中入力に配慮し、ネイティブの状態フィードバックを表示します。
- キャラクター、バブル、セリフ、表示状態の設定。検証済み追加パックと公式カタログブラウザーに対応します。
- Herdr のライフサイクル操作、保護付きの linked worktree 削除、インストール元を区別した明示的なアプリ更新。
- **capability 2：**統合された接続設定で Auto または登録されたプロバイダー（Herdr／Tern など）を選び、実際の接続状態を確認し、共有プロファイルへ稼働中の変更を保存します。現在の公開 Desktop Pet v0.4.3 はこの UI を含みますが、過去の Herdr v0.3.6 にはありません。2ホストの実アカウント／プロバイダー実行検証完了を意味しません。

## 動作要件

Apple Silicon Mac、macOS 13 以降。ソース構築には Rust/Cargo、Bun、Node.js/npm、Xcode Command Line Tools が必要ですが、プリビルドには不要です。ad-hoc 署名で、公証されていません。Herdr スナップショットは互換ローカルサーバー（0.9.0+）、返信は別途 `agent.prompt` が必要です。任意の client-attach 自動起動には公式 Herdr 0.9.3 に[連携所有のパッチとホスト手順](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)を適用するか、実際にフック対応のホストが必要です。標準 0.9.0/0.9.3 にはフックがなく、プラグイン導入はホストをパッチしません。

## インストールと任意の連携

中立 v0.4.3 formula は tap `6a9ae670cb3c69eb6e66bfd99ef351c513168f72` に公開済みで、実際の v0.4.3 Brew 新規導入/実行は保留中です。 導入コマンド:

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
# 公開済みの不変 v0.4.3 インストーラーと固定アセットを使用:
git checkout --detach v0.4.3
bash scripts/install.sh --prebuilt
# 同じ公開 v0.4.3 タグでソース導入を選ぶ場合:
# bash scripts/install.sh --source
# Replace with the installer-reported executable and your actual profile paths:
/absolute/reported/desktop-pet --config-dir /absolute/dev/config --state-dir /absolute/dev/state start
```

不変 v0.4.3 の既定／`--prebuilt` は正確な公開固定アセットを使用します（過去の v0.4.0／v0.4.1 は各 pin を保持）。取得／検証失敗をソース構築で隠しません。明示 `--source` は実際の公開 v0.4.3 タグを含む選択 core checkout を構築します。 standalone prebuilt は **実 OS アカウント home** の `Applications/DesktopPet.app`、承認された source は core の `dist/DesktopPet.app` を使います。admission は検証済み健全な既存所有者を再利用し得るため、推測した dist パスではなく導入器が示す実行ファイルを使います。共有 locator はアカウント home の `.local/share/desktop-pet/installation.json` に原子的に保存し、探索証拠であって権限ではありません。不正な明示パス／locator は fallback せず、実行ファイル、署名、capability、origin を再検証します。Brew は実際の formula／prefix を所有し、生存／予約中の導入を密かに引き継ぎません。

必要な連携だけを導入します。Herdr は薄い consumer です。Tern はネイティブアプリを取得・構築・インストール・自動起動しません。プラグインの更新・削除は共有アプリ、プロファイル、キャラクターを削除しません。通常のアプリ更新で Herdr プラグインを再導入しません。

```sh
herdr plugin install hanbong5938/herdr-desktop-pet
tern plugin install https://github.com/hanbong5938/tern-desktop-pet
```

## プロファイル・接続・移行

`--host auto|PROVIDER` は Auto または登録された不透明な提供者の選択で、固定 Herdr/Tern 列挙ではありません。観測選択の優先順位は CLI > `DESKTOP_PET_OBSERVATION_MODE` > 保存 `observation.host` > 新規 Auto です。旧 `both`→`auto`、host-less→Herdr は読み込み移行だけに適用します。実行中は Connection／`preferences set --observation-host auto|PROVIDER` で保存後に適用します。`--herdr-socket ABS` は登録 Herdr アダプターの backend オプションで core 制御 socket ではなく、アダプターなしでは登録しません。

新規プロファイルは Auto です。Connection は登録されたプロバイダーの実際の接続を示し、インストール有無を接続と扱いません。ID は不透明な値で、権限は名前の接頭辞やパスではなく capability で判断します。Auto は同時観測で、fallback ではありません。リモートは読み取り専用、Tern は返信のみで worktree 削除は不可です。受付はエージェントの完了ではありません。

[利用ガイド](docs/usage.ja.md)、[正確なプロファイル・探索契約](docs/development.md#profiles-and-installation-discovery)、[A→B 移行](docs/migrations/desktop-pet.md)を参照してください。status / Connection の実際の実行ファイル・config・state を Tern Configure に指定し Connect します。既存の Tern・beta・custom プロファイルはその場で保持し、マージしません。

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
