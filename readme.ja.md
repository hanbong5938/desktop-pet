# Desktop Pet

独立したネイティブ macOS デスクトップコンパニオンです。ルベリアはタッチに反応し、Herdr・Tern など登録済みプロバイダーのセッションを表示して、許可された送信先に返信します。ホストなしでも動作します。

[English](readme.md) · [한국어](readme.ko.md) · [日本語](readme.ja.md) · [简体中文](readme.zh-Hans.md)

**公開状態:** 既存の [Herdr v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6) は protocol 3 / observation capability 1 の公開版です。独立 Desktop Pet v0.4.0（protocol 4 / capability 2 / integration API 1）、bridge v0.3.7、中立 formula、連携タグは **未公開**です。実際の候補 package／VM 検査は公開導入の可否とは別で、最終受入表は未完了です。以下は中立 cutover の案内であり、検証済み公開導入ではありません。過去の文書、タグ、アセットは変更不可です。

<img src="assets/rubelia-thumbnail.png" alt="標準デスクトップコンパニオンのルベリア" width="220">

## 機能

- 移動・サイズ変更可能なデスクトップキャラクター。タッチや撫でる操作への反応、独立した状態バブル、メニューバー操作に対応。UI はシステム言語、韓国語、英語、簡体字中国語、日本語から選択できます。
- 観測した Herdr セッションのカード。検索・絞り込み・並べ替えと、ローカル／リモートの明確な区別。リモート観測は任意で読み取り専用です。
- 選択した**ローカル**エージェントセッションへの一行返信。IME の変換中入力に配慮し、ネイティブの状態フィードバックを表示します。
- キャラクター、バブル、セリフ、表示状態の設定。検証済み追加パックと公式カタログブラウザーに対応します。
- Herdr のライフサイクル操作、保護付きの linked worktree 削除、インストール元を区別した明示的なアプリ更新。
- **現在のソース限定：**統合された接続設定で Auto または登録されたプロバイダー（Herdr／Tern など）を選び、実際の接続状態を確認し、共有プロファイルへ稼働中の変更を保存します。公開 v0.3.6 にこの capability-2 UI は含まれません。

## 動作要件

Apple Silicon Mac、macOS 13 以降。ソース構築には Rust/Cargo、Bun、Node.js/npm、Xcode Command Line Tools が必要ですが、プリビルドには不要です。ad-hoc 署名で、公証されていません。Herdr スナップショットは互換ローカルサーバー（0.9.0+）、返信は別途 `agent.prompt` が必要です。任意の client-attach 自動起動には公式 Herdr 0.9.3 に[連携所有のパッチとホスト手順](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)を適用するか、実際にフック対応のホストが必要です。標準 0.9.0/0.9.3 にはフックがなく、プラグイン導入はホストをパッチしません。

## インストールと任意の連携

中立リリースと formula が実際に公開された後の公開コマンド:

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
# Requires neutral public assets; not yet published:
bash scripts/install.sh --prebuilt
# Explicit source alternative:
bash scripts/install.sh --source
# Replace with the installer-reported executable and your actual profile paths:
/absolute/reported/desktop-pet --config-dir /absolute/dev/config --state-dir /absolute/dev/state start
```

既定／`--prebuilt` は固定された中立公開アセットを要求し、現在は未公開です。失敗をソース構築で隠さず、明示 `--source` だけが現在の検証済み core を構築します。standalone prebuilt は **実 OS アカウント home** の `Applications/DesktopPet.app`、承認された source は core の `dist/DesktopPet.app` を使います。admission は検証済み健全な既存所有者を再利用し得るため、推測した dist パスではなく導入器が示す実行ファイルを使います。共有 locator はアカウント home の `.local/share/desktop-pet/installation.json` に原子的に保存し、探索証拠であって権限ではありません。不正な明示パス／locator は fallback せず、実行ファイル、署名、capability、origin を再検証します。Brew は実際の formula／prefix を所有し、生存／予約中の導入を密かに引き継ぎません。

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
- アプリ更新の自動確認は読み取り専用です。更新のインストール／適用には明示的な同意が必要です。更新機能の所有権 ACK は**完了を意味しません**。結果が不明な場合は再試行前に[アプリ内更新の復旧](docs/usage.ja.md#アプリ内更新)と [CLI 更新状態と復旧（英語）](docs/cli.md#app-update-status-and-recovery)を参照してください。

## 設定とトラブルシューティング

プロファイル優先順位は **明示的 `--config-dir`/`--state-dir` > `DESKTOP_PET_CONFIG_DIR`/`DESKTOP_PET_STATE_DIR` > default-profile record > 中立 XDG 既定値**（未指定なら `~/.config/desktop-pet`、`~/.local/state/desktop-pet`）です。core は `HERDR_PLUGIN_*` を暗黙使用せず、Herdr wrapper だけがホスト注入を明示引数に変換します。config は lifecycle/preferences、characters、menu-bar-icons、state は lock、私有 `control.sock`、daemon log、更新証拠を保持します。実際の両パスを合わせてバックアップします。`--herdr-socket ABS` は登録 Herdr アダプターの backend オプションで制御 socket ではなく、アダプターなしで core は正規化／登録しません。[正確なプロファイル・探索契約](development.md#profiles-and-installation-discovery)。

Core は共通 lifecycle 保存と `auto_start`（既定 on）を所有し、追加キーは登録連携が所有します。Herdr 所有者は **`lifecycle.json` が存在しない場合だけ**、最初の lifecycle 書込みで共有 profile writer 内に `exit_with_herdr:true` を原子的に初期化します。既存ファイルにキーがなければ false、明示的 false も維持します。旧真偽値 `enabled` は false を含め `auto_start` に移行します。端点、起動メタデータ、他所有者のフィールドは保持し、`preferences.json` ではなく `lifecycle.json` に保存します。不正 JSON／値は既定化せずエラー、保存失敗は既存ポリシーを維持します。Auto／アダプターなしは Herdr がなくても動作を維持し、以下の Herdr 端点／切断規則は Herdr 専用選択だけに適用します。

## 資料とウェブサイト

- [使い方と復旧](docs/usage.ja.md) · [開発とインストール（英語）](docs/development.md) · [ネイティブ自動化 CLI 契約（英語）](docs/cli.md#english) · [パッチ済みホスト連携（英語）](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)。
- [リリース履歴（英語）](docs/releases/README.md) · [v0.3.6 の変更点（英語／韓国語）](docs/releases/v0.3.6.md) · [v0.3 更新ガイド（英語）](docs/releases/0.3.md#upgrade-from-v021-or-beta) · [過去のベータ／プレビュー案内（英語）](docs/migrations/unreleased.md) · [v0.1.11 → v0.2.0 移行（英語）](docs/migrations/v0.1.11-to-v0.2.0.md)。
- 宣伝用の[日本語](https://hanbong5938.github.io/desktop-pet/ja/)・[韓国語](https://hanbong5938.github.io/desktop-pet/)・[英語](https://hanbong5938.github.io/desktop-pet/en/)・[簡体字中国語](https://hanbong5938.github.io/desktop-pet/zh-Hans/)ウェブサイトはこの**ネイティブ**アプリを紹介しています。ブラウザー内でアプリを動かすものではありません。中立 Pages の公開はここでは未確認です。ウェブサイトの開発／公開は [GitHub Pages ガイド（英語）](docs/development.md#website-github-pages)を参照してください。別の[キャラクターギャラリー](https://hanbong5938.github.io/herdr-characters/)では追加パックを紹介しており、対応言語は英語と韓国語のみです。

## アートとライセンス

[ソフトウェアライセンス](LICENSE.txt)とアート／モデルの利用条件は**別物**です。MIT ソフトウェアライセンスによってキャラクターのアートが再ライセンスされることはありません。同梱ルベリアの[ライセンス](assets/rubelia-default/LICENSE.txt)、[帰属表示](assets/rubelia-default/ATTRIBUTION.txt)、[出典記録](assets/rubelia-default/source-record.json)、[背景修復記録](assets/rubelia-default/background-repair.json)が正式な情報です。`LICENSE.txt` には元の Coding Cat の告知も残っています。そのアート／ソースは[キャラクターリポジトリ](https://github.com/hanbong5938/herdr-characters)へ移されました。各追加パック独自の条件を必ず保持してください。アート所有者の承認はモデルの権利を与えません。Qwen の研究用ライセンスは制作記録であり、モデルのエンジンや重みは同梱されていません。[キャラクター制作とライセンス（英語）](docs/development.md#character-authoring)を参照してください。
