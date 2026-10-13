# Desktop Pet — Herdr integration

[English](readme.md) · [한국어](readme.ko.md) · [日本語](readme.ja.md) · [简体中文](readme.zh-Hans.md)

**公開状態:** 既存の [Herdr v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6) は protocol 3 / observation capability 1 の公開版です。独立 Desktop Pet v0.4.0（protocol 4 / capability 2 / integration API 1）と移行用 bridge v0.3.7 はレビュー済み候補で、公開インストール・実行検証は未完了です。以下は中立製品への切り替え手順であり、文書・タグ・ソース版番号は公開の証明ではありません。

必要な連携だけを導入します。Herdr は薄い consumer です。Tern はネイティブアプリを取得・構築・インストール・自動起動しません。プラグインの更新・削除は共有アプリ、プロファイル、キャラクターを削除しません。通常のアプリ更新で Herdr プラグインを再導入しません。

[Desktop Pet / core app](https://github.com/hanbong5938/desktop-pet/blob/main/readme.ja.md) · [Herdr host / patch / OMP / link](integrations/herdr/README.md) · [Legacy A → B migration](https://github.com/hanbong5938/desktop-pet/blob/main/docs/migrations/desktop-pet.md)

中立リリースと formula が実際に公開された後の公開コマンド:

```sh
brew install hanbong5938/tap/desktop-pet
herdr plugin install hanbong5938/herdr-desktop-pet
herdr plugin action invoke start --plugin desktop-pet
```

Apple Silicon Mac、macOS 13 以降。ソース構築には Rust/Cargo、Bun、Node.js/npm、Xcode Command Line Tools が必要ですが、プリビルドには不要です。ad-hoc 署名で、公証されていません。Herdr スナップショットは互換ローカルサーバー（0.9.0+）、返信は別途 `agent.prompt` が必要です。任意の client-attach 自動起動には公式 Herdr 0.9.3 に[連携所有のパッチとホスト手順](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)を適用するか、実際にフック対応のホストが必要です。標準 0.9.0/0.9.3 にはフックがなく、プラグイン導入はホストをパッチしません。

プリビルド・ソース導入は Herdr 連携ではなく **core** チェックアウトで実行します。既定はプリビルドのみで、取得・検証失敗からソースビルドへ黙って切り替えません。`--source` は現在の core だけを構築し、その `dist/DesktopPet.app` を使用します。

> Native app features below are owned and documented by the independent core, not built in this integration checkout.

## 機能

- 移動・サイズ変更可能なデスクトップキャラクター。タッチや撫でる操作への反応、独立した状態バブル、メニューバー操作に対応。UI はシステム言語、韓国語、英語、簡体字中国語、日本語から選択できます。
- 観測した Herdr セッションのカード。検索・絞り込み・並べ替えと、ローカル／リモートの明確な区別。リモート観測は任意で読み取り専用です。
- 選択した**ローカル**エージェントセッションへの一行返信。IME の変換中入力に配慮し、ネイティブの状態フィードバックを表示します。
- キャラクター、バブル、セリフ、表示状態の設定。検証済み追加パックと公式カタログブラウザーに対応します。
- Herdr のライフサイクル操作、保護付きの linked worktree 削除、インストール元を区別した明示的なアプリ更新。
- **現在のソース限定：**統合された接続設定で Auto または登録されたプロバイダー（Herdr／Tern など）を選び、実際の接続状態を確認し、共有プロファイルへ稼働中の変更を保存します。公開 v0.3.6 にこの capability-2 UI は含まれません。

### Tern 観測：ソース限定 exchange v2

Auto が Tern を選択した場合、または Tern を明示的に選択した場合、現在の exchange-v2 ソースはローカルエージェントだけでなく、接続済みの全 Tern SDK ホストを自動観測します。カードにはホスト名とローカル／リモートのラベルが付き、各ホストの Offline 状態と観測寿命は独立しています。Tern のホスト観測は Herdr の local/remote/machines 範囲設定とは別で、新しいホスト選択 UI はありません。返信はローカル限定で、リモートカードには送信できません。

これは公開 v0.3.6 の動作ではなく、当時のローカル限定 Tern 経路は変更されません。実際のリモート SDK エージェントと、対応するネイティブ／プラグインのインストールは**未検証**です。現在のプラグインが要求する中立 `DesktopPet.app` のインストール仕様は、このソースブランチの `HerdrDesktopPet.app` バンドルとの互換性を証明しません。完全なソース契約と検証範囲は[使い方](docs/usage.ja.md)と[開発 / 検証根拠](docs/development.md)を参照してください。


## Source / link

Install or build the core first (`bash scripts/install.sh --source` in the **desktop-pet core** checkout). Then link **this integration checkout**, not the core. `plugin link` does not run a build. The integration wrapper discovers a verified compatible native executable, preserving the exact profile/socket provenance; it never compiles a native app here. Client-attach startup remains optional and requires the actual patched host/hook; plugin install does not patch Herdr. See [separate checkouts](integrations/herdr/README.md#build-in-separate-checkouts).

## Ownership

This repository owns `native-adapter`, `herdr-plugin.toml`, thin install/run wrappers, `integrations/herdr` and `integrations/omp`, integration validation, and frozen historical releases. App/UI/rig/artwork/packaging/source instructions belong to [desktop-pet](https://github.com/hanbong5938/desktop-pet). `.herdrchar`, pack v5, `default@0`, rig ABI and [herdr-characters](https://github.com/hanbong5938/herdr-characters) catalog/gallery authority are unchanged. Historical [v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6) and older release bodies/tags/assets remain immutable; an integration-only v0.4.0 does not carry a native archive.

## アートとライセンス

[ソフトウェアライセンス](LICENSE.txt)とアート／モデルの利用条件は**別物**です。MIT ソフトウェアライセンスによってキャラクターのアートが再ライセンスされることはありません。同梱ルベリアの[ライセンス](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/LICENSE.txt)、[帰属表示](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/ATTRIBUTION.txt)、[出典記録](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/source-record.json)、[背景修復記録](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/background-repair.json)が正式な情報です。`LICENSE.txt` には元の Coding Cat の告知も残っています。そのアート／ソースは[キャラクターリポジトリ](https://github.com/hanbong5938/herdr-characters)へ移されました。各追加パック独自の条件を必ず保持してください。アート所有者の承認はモデルの権利を与えません。Qwen の研究用ライセンスは制作記録であり、モデルのエンジンや重みは同梱されていません。[キャラクター制作とライセンス（英語）](https://github.com/hanbong5938/desktop-pet/blob/main/docs/development.md#character-authoring)を参照してください。
