# Desktop Pet — 利用ガイド

独立したネイティブ macOS デスクトップコンパニオンです。ルベリアはタッチに反応し、Herdr・Tern など登録済みプロバイダーのセッションを表示して、許可された送信先に返信します。ホストなしでも動作します。

[English](../readme.md) · [한국어](../readme.ko.md) · [日本語](../readme.ja.md) · [简体中文](../readme.zh-Hans.md)

## 現在の公開状態と予定の単一承認経路

**過去の起動欠陥と恒久修正:** 所有 VM の実際の公開 v0.4.11 履歴実行は `Migration history: current native daemon does not prove the migrated image/profile readiness` で失敗しました。新 receipt・SDK 配信はなく SDK 0.1 は `/opt/homebrew/bin/herdr-desktop-pet` 不在エラーのままでした。UI Ready 前の観測エラーが保持 operation を失わせました。v0.4.12 は read-only discovery で Ready 前に operation を保持し、既存の認可済み observation/poll が後で配信します。current image/owner/profile・signed-stage・正当な runtime 起動・Stop・Unknown guard を維持し、旧 manager 再実行・plan/journal 書換え・手動再接続はしません。対象ソース回帰の成功は end-to-end 成功ではありません。

**別の fixture 前提:** 診断 fixture には対応 Tern.app CLI の両配置がありませんでした。親は所有 VM だけに真正な完全 SDK app を導入し、default old consumer/KV は変更していません。この導入は source 修正でも配信・binding 証拠でもありません。tools Mac・VM はアクセス不能の問題の別コンピュータではなく、host 導入 app を変更したりその復旧を主張しません。


**現在の後継: Core v0.4.12 / genuine bridge v0.3.16 — 適格性確認・公開は未完了です。** Core v0.4.11 は release `414563938`、公開時刻 `2026-10-16T05:16:03Z`、source `7d5215f4b4a2442d18438b453cc5afae14c2f4a0` から公開され、公開6資産を独立検証済みです。別 Wiki job は `WIKI_PUBLISH_TOKEN` がありませんでした。genuine bridge v0.3.15 CI `38855045701` は成功しましたが tag/release は未公開です。公開 provenance は過去移行修復の受入証拠ではありません。 See [successor release notes](releases/v0.4.12.md) for the pending correction and immutable predecessor failures. Consumer metadata remains Core v0.4.8 until actual public successor materialization; Tern production stays `69696023e00a0fcda8b85b332a075bfa26d6c26d`.

**予定の通常経路:** 最初の承認1回 → 正確に同じ profile の `migrationReady` / Commit → 更新 Tern consumer の配信・ロード → SDK optimistic identity guard と atomic canonical save → 実際の同じ Tern ウィンドウの fresh binding → 自動 native handover receipt → 承認済み旧所有者の retirement。**実際の再接続と receipt は retirement より先**です。保存や ACK は再接続ではなく、この SDK guard は真の cross-VM transactional CAS ではありません。別の B/source review/Migrate 承認や手動 Configure → Save → Connect / Resume は通常経路に追加しません。複数 profile では影響を受ける各 target が start・Commit・rebind する必要があり、旧 blanket live-profile guard だけでは完成しません。独立 profile は統合しません。

**正確な旧 source 承認境界:** 実際の導入済み v0.3.10/v0.3.11 を含む stable protocol-3 predecessor v0.3.9–v0.3.14 の真正な元承認を対象とします。保持された matching signed helper・plan/result・解放済み lease・確定した成功/quiescent manager 結果が正確な **現在の v0.3.16 candidate**、image、installation root/owner、元 config/state profile を認証し、Stop fence が不変である必要があります。過去の v0.3.11 candidate Completed は現在の移行許可ではありません。Unknown・prerelease/未知 source・protocol 2・hash 変更・無関係な profile・利用者 Stop は fallback 許可になりません。v0.3.6 と凍結 protocol-2/beta entry は別の互換経路です。追加 B/Migrate 承認や UI acknowledgement は元承認の代用になりません。

**Commit 済み履歴の修復と失敗の区別:** 実際の othermachine 問題は Commit 済み `AwaitingReconnect` 後の古い保存パス `/opt/homebrew/bin/herdr-desktop-pet` であり、Commit 失敗や manager 再実行の証拠ではありません。same-image / 適格 successor 修復は旧 plan/journal をバイト単位で不変に保ち、新 repair/handover receipt だけを追加します。manager replay・手動 Configure・手動 Resume は行いません。実際の旧証拠・helper・reservation・config/state・資格情報・provider/account データ・prompt/transcript・無関係な profile を保持します。definite no-spawn、Commit 後の未再接続、確定した quiescent retirement 失敗、typed cancellation、Unknown を区別します。Unknown は clear/replay も cancelled への変更もしません。実際の利用者 Stop は rollback を含む再起動を禁止します。必要な全 binding と native receipt までは retirement を阻止します。文書や旧実行は現在の tools Mac の状態を証明しません。

**過去の観測境界:** C1 署名 bundle（neutral 72 / legacy 68 ファイル）と instance `5e2cdb3d4178a864d9b4e995d0b3e763`、sequence `37` の実 SDK/native binding は C1 の証拠ですが、headless から隔離されず GUI source への帰属は未証明です。別途 source `7c40127abf79603ca68837f39c6dca362b2acc7c` と変更のない Tern source `69696023e00a0fcda8b85b332a075bfa26d6c26d` では、実 GUI/frame-driven sequence `47`、通常の native Stop/Start 後の新 sourceWindow・sequence `13` を Connect・保存 Configure・source patch なしで観測しました。GUI は自然な `signed_out` でした。自律的 quiet-background scheduling・認証済みモデル配信・migration・retirement・別コンピュータの復旧、または v0.4.12 の通過証拠ではありません。tools Mac は問題の導入先ではなく、その代わりに host の導入済み app を変更しません。

## 過去の公開・実行記録 — 予定の通常経路ではない

以下の版・PID・SHA・取消/Unknown 観測・受入れ範囲は元の歴史的範囲を保持します。当時の「現在」という表現や別 A/B 承認・手動接続手順は予定の単一承認経路の指示ではありません。

**Core v0.4.7:** [Desktop Pet v0.4.7](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.7), release `413584934`, source `52514da5115f09aac22de5429c08f7acdd993258` (2026-10-15T08:39:17Z). Six anonymous assets, strict/deep signatures, protocol 4 / observation 2 / API 1 and exact 68-file closure passed. Current observed acceptance: [v0.4.7 ledger](releases/v0.4.7.md). Pages is not yet observed; Wiki publication lacks dedicated `WIKI_PUBLISH_TOKEN`.

**過去の v0.4.7 経路（置換済み、当時は公開 Brew A′/B 未観測）:** 実行中 v0.3.10 → 独立 native 同意の通常 v0.3.11 更新 A′ → 保持した元の対応する署名 helper で terminal/quiescent settlement を確認 → 別の source review と最終表示 native **Migrate** 同意で Core v0.4.7 B → 正確な profile の ready/Commit → 実際の所有 manager による retirement。当時は A′ と B に別同意が必要でした。v0.3.6 互換経路は別で、beta は先に凍結した一致 v0.3.6 pair が必要でした。

**同意と復旧:** v0.4.7 は bound typed Cancelled と VisibleApproved を区別します。予期しない native 応答・EOF・invalid/foreign/nonzero・不整合 delivery は Unknown のままです。Unknown manager を clear/replay したり cancelled に変更しません。ユーザー Stop は rollback start を含む再起動を禁止します。元 helper/journal/reservation・正確な config/state・無関係な profile を保持し、target ready/Commit 後にレビューした所有 source だけを retire します。v0.4.6 final-cancel false-Unknown 証拠は不変の過去記録です。現行の手動 candidate→公開 v0.4.7 観測は ledger を参照。公開 Brew A′/B・ManagerUnknown・late Stop・definite retirement failure/Resume は未観測です。

**Historical execution records / 과거 실행 기록 / 過去の実行記録 / 历史执行记录:** The older operations below retain their original versions, PIDs, hashes and Unknown scope; they are not v0.4.7 / v0.3.11 runtime acceptance.

**実公開 Brew v0.4.4 → v0.4.5 通常更新完了:** operation `a6991e69233377c32b150186c62888fd`、`homebrew_upgrade` / `manager_exited`、installed/applied true。PID `10377` / instance `37953e017fc9208bfcc785a4ff138494`、Brew 派生 image `a06f7e7045a1cfa83a3d7ed7d7a135eab4f8d911a3275647b89743fe5815c500` は正確な旧 private profile・保存 Herdr(H1/T0)で UI/control ready に達しました。更新後 native Check は緑の **No update from selected source / 0.4.5 / Homebrew** です。適用前 Check は source preferences のみ変更し、適用は `source.log` と新 operation ファイル以外の旧 config/evidence・無関係な既定 tree を保持しました。無関係な旧 hidden PID `2641` は旧 A/B 証明を封印後に明示 Stop しました。Herdr 再導入はありません。 Actual manager `/opt/homebrew/bin/brew`: PID = PGID `9853`, exit 0, drained true; retained helper strict-signature result 0, stdout upgraded only desktop-pet v0.4.4 → v0.4.5.

**手動所有者修復（自己更新ではない）:** 停止した公開 v0.4.3 checker は自己更新していません。署名済み元 app バックアップ・明示 Stop `9340`・固定公開 v0.4.5 prebuilt 導入・自動起動なし検査・明示 Start 後、実 PID `13055` / instance `93f6048e2ae1266c9d5a42ede0f2da51` は元 profile・保存 Herdr H0/T0・正確な公開 native image `693bad6245d28be4350b661f018e34416ca950b37f49a6a4a3967c5d1a67eacb` で ready です。Start transport は20秒 timeout、再試行せず、その後に実 GUI/readiness を観測しました。daemon log 以外の旧 config・history 321ファイルは保持。最終 native GUI Check は緑の **No update from selected source / 0.4.5 / Standalone installation**、元 v0.4.2 history は保持。最終 Check は期待された preferences のみ変更し、Start 再試行・Unknown journal replay はありません。

**実際の新規公開 managed v0.3.6 → v0.3.10 A → v0.4.5 B: COMPLETE.** Genuine private Herdr 0.9.3 / protocol 22 で正確な凍結 v0.3.6 pair を導入。Native Check は source preference check metadata のみ変更し、導入/process・他の4 tree bytes は保持。別途 A 同意で operation `20cca91548584437af112bb04cc260b0` 完了、保持した署名 v0.3.6 helper は B 後も completed / manager_exited / installed=true / applied=true。正確な公開 bridge v0.3.10 PID `2894` / instance `8f8f97c36aed6625166a679396596a66`、raw image `0b1c8e63c6cee6904208bc3d90f184806c8f1216b8114a2b30da009d92be65a6`、protocol 3 / observation 1 / API 1・実 native UI/control ready・Herdr 1 / Tern 0 に到達。Bridge Check は A を確定し、緑の **No update from selected source / 0.3.10 / Herdr plugin manager** を観測しました。

別途 Move preview・strict/deep 検証済み公開 v0.4.5 protocol-4 native **Migrate** 同意は B `6ee850aa5577314c6d9cf6ebc7eae981` を1回承認（plan SHA-256 `b6fe929da173e648fd2ad8e2845b7fda00461e3e6002e7bb9113155d403127de`）。Durable phase=complete、native_ready=true、target_committed=true、error=null。Source `2894` が Stop を確認後、Core v0.4.5 PID `3594` / instance `2ee0995b8b3dc3abe18e6238a91929d9` は同じ元 private config/state・保存 Herdr・Herdr 1 / Tern 0 で実 UI/control ready。適格な Brew image `a06f7e7045a1cfa83a3d7ed7d7a135eab4f8d911a3275647b89743fe5815c500` を再利用し、`target_manager:null`、中立 install/upgrade manager は実行なし。Genuine Herdr retirement PID = PGID `3648`、exit 0、quiescent=true で不変 integration v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8` を導入。Enabled registry・clean checkout は正確な ref、21 actions/startup/client.attached があり、旧生成 app は不在、新 integration tag はありません。4封印 tree の全既存通常ファイル（source config 5/state 164、無関係な既定 config 5/state 2）・**元 A ファイル87個**・helper/plan/records は B 後 byte-identical、**旧 daemon-log の例外もなし**。新 B ファイルは追加のみ。無関係な既定 PID `1971` / instance `27752246b4da5c6f3964a74dd22881a2` は running+Hidden・Auto/H0/T0 のまま、Stop/restart/merge なし。Adoption は private XDG namespace 内のみ。B 後 native settings は exit 0・同じ ready instance。実際の移行 PID `3594` の native B 後 Check は緑の **No update from selected source / 0.4.5 / Homebrew**、migration/legacy retirement `6ee850aa5577314c6d9cf6ebc7eae981` 完了と併せて確認。Source `preferences.json` check metadata のみ変更し、元 A ファイル87個・全 B proof を含む現在の state 239ファイルと無関係な既定 tree は byte-identical です。Pages 配備・公開4言語 render は別途観測検証が必要で、native 受入れ/source 編集は配備証明ではありません。 下記 v0.3.9/v0.4.4 operation は過去の証明です。

実固定 beta route は **公開 v0.3.9 entrypoint で PASSED**: 元 protocol-2 archive `d49824778dc0d898b4386cc04a4fbed34da40ebfe990148441d365984555d1aa`、live/direct-bridge guard・凍結一致 v0.3.6 pair・自動起動なし・明示 ready Start・署名旧 helper バックアップ。non-app Git 38ファイル・user edits・profile は保持。v0.3.10 beta entrypoint 実行ではありません。history 検証は private 読取専用構造検査と正確な現行 strict fresh read/write/execute/startup/resume authority を分離します。修正前失敗の回帰2件・coordinator test 70件が通過し、実 v0.4.5 読取専用旧 history probe で config・state 321ファイルのハッシュは不変です。旧 Unknown は元 scope/evidence に束縛された読取専用で、clear・replay・promote しません。

**実際の新規公開 Brew v0.4.5 導入通過:** 導入前に全中立 app/config/state/process root の不在を記録しました（74ファイル / 44.3 MB、内外の再署名5個、他 formula 更新なし）。適格な読取専用 helper discover は正確な Brew 派生 image `a06f7e7045a1cfa83a3d7ed7d7a135eab4f8d911a3275647b89743fe5815c500` を検証し、導入・検査は中立 config/state・standalone app・process を作成しませんでした。**明示 Start 後のみ**既定 PID `1971` / instance `27752246b4da5c6f3964a74dd22881a2`、v0.4.5 / protocol 4 / capability 2 / API 1・Auto / H0/T0・実 UI/control ready に達しました。実 Settings は Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0・内蔵 Rubelia を表示。明示 Hide は同じ既定 instance を hidden・running に保持し、完了した新 A/B とは無関係です。新規導入は managed A/B 受入れではありません。

**過去の v0.4.4 新規 Brew 導入証明:** 中立 v0.4.4 formula は tap `2aa9a4be9517e0232a3f2b8e2b3c748045c69586` に公開済みです。適格な Brew v0.4.4 新規導入は通過しました（74ファイル / 44.3 MB、内外の再署名5個）。認証済み検査で Homebrew 派生 image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312` を確認しました。導入・検査は中立 config/state・standalone app を作成せず、デーモンも起動しませんでした。明示的な手動 **Start** の後だけ実際の v0.4.4 UI/control ready、PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470` に達しました。ルベリアと実際の Settings は **Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0** を表示し、その後の明示 Hidden は同じ既定 instance を保持しました。新規導入の証拠であり、v0.4.4 通常更新や managed 移行の完了証明ではありません。

**Historical public bridge v0.3.11 publication:** [Genuine Herdr bridge v0.3.11](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.11), release `413623057`, tag object `a907424e89c10d484f8646f41d5f020fa93ba8bc`, built from exact public Core `52514da5115f09aac22de5429c08f7acdd993258`. Five anonymous assets, genuine legacy bundle/executable/helper, protocol 3 / observation 1 / API 1 and exact 68-file closure passed. At that publication, stable tap `6693cd9db26e6dba9a257bbe76acaf78cb502c2f` served Core v0.4.7 / bridge v0.3.11; beta was unchanged.

**過去の v0.3.9/v0.4.4 managed A 完了:** 凍結 v0.3.6 Check は公開 v0.3.9 を選択し、`preferences/last_update_check` のみ変更しました（`1791963743` → `1791965310`）。Check 中は他の4 tree bytes・source PID `2467`・無関係な既定 PID `2641` が不変です。正確な source `0f33f28` の native A 同意で operation `d5b2bf6f02e031eabb5a8c8fdd805cab` が完了しました（`completed` / `manager_exited`、installed/applied true）。実際の公開 v0.3.9 native PID `3417` / instance `b42b25d48315e66bd87e7685e58cfb14`、image `0ed12d159b6bbb83ada833d39b4524ac5f7eccb1253b7acaf9aa2b71cbf833ce` は正確な旧 private config/state で一致 protocol 3 / capability 1 / API 1・UI/control ready・Herdr 1 / Tern 0 に達しました。実 managed source の Git status は完全に clean です。保持した元の v0.3.6 helper の strict/deep署名と実際の completed status も通過しました。baseline は旧 A 証拠87個と全 state ファイル91個を hash で保存します。

**過去の v0.3.9/v0.4.4 managed B 完了:** 別の Move preview と独立 strict/deep 検証済み公開 v0.4.4 coordinator が native **Migrate** 同意を受け、operation `d729fd740f4e459945edda00bf48d944`、plan SHA-256 `6471e928254229a66a6fc64a88f67095777d6bd890d621feb36b08a4c7d1fc69` で **COMPLETE** に達しました。Native Prepare/Stop で公開 v0.3.9 PID `3417` / instance `b42b25d48315e66bd87e7685e58cfb14` を停止し、適格な既存 Brew v0.4.4 app root/image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312` を使用しました。`target_manager:null` のため **再導入はありません**。Target PID `4104` / instance `8912b3aad5419b1c14a78ef3418ae36f` は正確な旧 private config/state と保存 Herdr 選択（Herdr 1 / Tern 0）を維持し、protocol 4 / capability 2 / API 1・実 UI/control ready に達しました。native_ready・target_committed は true です。実 Herdr retirement attempt 1 は PID = PGID `4140`、exit 0、quiescent true、error null です。実 plugin list は不変 Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8` の enabled integration-only Desktop Pet を表示し、managed Git HEAD も一致しました。実 manager 出力は v0.4.1・21 actions・startup/client-attached hooks です。Managed dist/app と旧 PID `3417` は消え、standalone `DesktopPet.app` は作成しませんでした。正確な retired v0.4.1 タグの歴史的 Core v0.4.2-minimum wrapper は同じ v0.4.4 PID/instance を実際に再利用しました。新 app・新 tag はありません。元の A 証拠87個の SHA-256 と既存旧 source-config hash は全て不変、無関係な既定 config/state の両 tree は byte-identical です。hidden の既定 v0.4.4 PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470` は不変で稼働中です。Private XDG base profile を adopt し、global default と merge していません。実 GUI は Desktop Pet / 内蔵ルベリア・Settings **Herdr Connected 1 / Tern Disabled 0** と視覚確認し、clean-image capture のみ保留です。Fault のない最終公開 v0.3.9/v0.4.4 A/B happy path は、別途通過した過去 working metadata `f7edf5e` の definite failure/明示 Resume 証明と区別します。

**過去の v0.4.7 consumer / SDK 記録:** Herdr integration は当時 assetless v0.4.1 (`6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`) でした。local consumer scripts/evidence 全8件は公開 Core v0.4.7 由来で、当時 main push・新 integration tag・consumer native build はありませんでした。手動 same-source v0.3.11 candidate → 公開 Core v0.4.7 B は通過しましたが公開 Brew owning-manager retirement の証拠ではありません。当時は同じ executable/config/state の Configure → Save → Connect が必要で account/provider・両起動順の受入れは未完了でした。[過去の観測](releases/v0.4.7.md)は現在の Mac 状態の証拠ではありません。

**Historical v0.4.5 consumer evidence:** **consumer・SDK 境界:** Herdr 連携は公開 v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`（assetless release `412252863`）を維持します。歴史的 Core v0.4.2 minimum は凍結され、健全な Core v0.4.3 再利用は観測済みです。現在の Herdr consumer scripts と metadata/evidence 全8ファイルは、公開 Core v0.4.5 release `412667385` / ソース `51dcf22415b1ff0faf07651cb83c1c696c980f43` から実際に materialize されました。新 Herdr タグはなく、最終公開 managed retirement と不変 wrapper の v0.4.4 再利用は上記の実 B 結果で別途確認しました。Tern v0.2.0 と互換 Core v0.4.1 pin は不変です。Tern は検証済み実行ファイルと実際の config/state 組を明示 **Configure → Save → Connect** で接続し、アプリを導入・自動起動しません。実 Tern keychain/provider sign-in と genuine OMP 18.10.3 providerSignIn/NoModel は両起動順序の同一 instance/prompt/transcript/reconnect 受入れを引き続き阻止しています。資格情報の複製・偽アカウント・ACK・完了で代替しません。**全 matrix は完了していません。** 実際の現行 Herdr v0.4.5 reviewed-consumer VM smoke は通過: materialized wrapper は同じ Brew v0.4.5 PID `10377` / instance `37953e017fc9208bfcc785a4ff138494` を発見。Hide 後 ensure は bootstrap なしで Hidden / visible false・同じ instance を保持し、明示 Show で復元しました。現在の Herdr source 検証通過: integration-only ownership v0.4.1・shell/manifests/TypeScript・固定 contracts `85c644cd90d8c6a9e97e9dc313cc4750f344235f`・**Rust adapter test 104件と含まれる test の child 再実行1件**（105件の別 test・Bun ではない）。Native app/release 構築なし。

**過去の v0.4.2 / v0.3.7 根拠（v0.4.4 実行証明ではない）:** **公開状態:** [Desktop Pet v0.4.2](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.2) は公開済みです（release `412241744`、`2026-10-14T01:09:50Z`、不変ソース `cb10ce2c3cc65402cbb914d60301f7627d5546d5`）。匿名6アセット、strict/deep署名、protocol 4 / capability 2 / integration API 1、配備68リソースの全ハッシュと正確な closure を検証しました。tap `32919c745864b54a2e716fcc5a3714d0d38d3e84` の中立 formula と実際の公開 v0.4.1 → v0.4.2 Brew 更新で receipt/derivation、署名5個、`brew test`、Auto のネイティブ UI/control ready（Herdr 0 / Tern 0）を確認しました。新規公開 standalone v0.4.1 導入と同時インストーラー2個による稼働中所有者の再利用を確認し、ネイティブ Check が実際の公開 v0.4.2 を提示、同意後 `standalone_replace`、`manager_exited`、`installed:true`、`applied:true` に到達しました。同じ config/state と正確な公開 v0.4.2 image で準備完了です。Check は locator/profile/lifecycle/locks/PID を保持しましたが `preferences.json` の確認キャッシュは変化したため、全設定バイト不変ではありません。[修正 integration-only Herdr v0.4.1](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.4.1) も公開済みです（release `412252863`、不変 ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`）。公開 wrapper 2個は非表示のまま同じ Brew v0.4.2 PID/instance/image を再利用し、明示 config/state と実際の private patched Herdr 0.9.3 socket で Auto Herdr 1 / Tern 0 に到達しました。新規環境の公開 bootstrap 2個は OS-home のアプリ1個を導入/再利用し、連携内 native ファイルやデーモン起動はありません。これは公開 Herdr manager 導入 receipt の証拠ではありません。元の Herdr v0.4.0 の生成アプリ/失敗テスト scaffold symlink ソース欠陥は保持し、integration-only だったとは主張しません。[Bridge v0.3.7](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.7) も公開済みです（release `412267469`、不変 tag `987a4b859f95e922c1c5443ead372d10d0dddb24`）。正確な Core `cb10ce2` と修正 Herdr `6d2ac04` から構築し、匿名5アセット・署名・68ファイル closure を検証しました。tap `657f32cc4cc563ae40ae0432ac5123dcb3f58426` で old stable formula のみ v0.3.7 に更新、beta は不変です。manual-origin の private 実ホスト実行は Herdr 1 / Tern 0 ready で、managed A→B の証拠ではありません。実際の凍結公開 Brew v0.3.6 → v0.3.7 A `6a10663c2d2f0177990623a969238af4` と別途同意した Core v0.4.2 B `c66961f9e5a91c0023545065ae61385b` は、実際の Brew retirement（exit 0、quiescent）まで完了しました。B 前後で全設定ファイルのハッシュと旧 A 証拠87ファイルは不変で、実際の Core v0.4.2 UI は Desktop Pet / Herdr Waiting 0 / Tern Disabled 0 です。一方、実際の managed v0.3.7 A `a1aec5902020de7bcaaf583b1a5f50a5` は native `/tmp` プロファイル検証に失敗しました。checkout 置換・registry 登録前に manager exit 1 となり、凍結 helper は Unknown / `manager_exited`、installed/applied false を保持しています。VM・journal・reservation は recovery/replay なしで保存されています。

**過去の公開 v0.3.8 managed 結果・安全な successor 境界:** managed v0.3.6 → 公開 v0.3.8 A は完了し（`16b6307413ec409efe3eb5a04c9cc21f`）、元の署名済み v0.3.6 helper が completed status を確認しました。明示 Check 後の B preflight は `managed source changed; migration never resets user changes` で拒否しました。`dist/HerdrDesktopPet.app` の生成ファイルちょうど68個が untracked でした。B 同意・Stop 前のため B manager・fault flag はなく、当時の source PID `3563` は不変でした。元の公開 v0.3.8 タグは通常 UTF-8 Git ファイル10個で `.gitignore` はありません。B 制限は歴史的事実で、公開 v0.3.9 が古いタグを遡って修正するものではありません。手動 ignore・`git clean`・reset で旧タグを通してはいけません。managed v0.3.8 A 完了済みの利用者は、保持した **一致する署名 helper** で既知の terminal 旧 operation を確認し、実 config/state・旧証拠をバックアップし、source を明示 **Stop** してから **実際の所有 Herdr manager** で successor を明示導入します。Unknown manager operation は絶対 replay せず、VM・journal・reservation を保持します。

**過去の working-metadata・通常更新証明（最終 v0.3.9/v0.4.4 受入れではない）:** genuine managed B は working metadata `f7edf5e0eba374248b9a5ee9d66d26109b39eca2`・不変の公開 v0.3.8 native・公開 Core v0.4.3 で NativeReady/Commit 後に definite retirement failure（manager exit 1、quiescent）となりました。導入した所有 `uchg` flag のみを除去し、同じ plan SHA `3615fa140fd1b0d49910f5f2ef170b64122243c6c65d0098d4ecfd729e951f16` に独立の明示 Resume 同意を得て attempt 2 が完了しました（実 CLI exit 0、quiescent）。旧 managed app/source は retirement され、Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8` が同じ v0.4.3 instance を再利用しました。旧 A 証拠87個・first-attempt/plan/consent 5個・旧 config・無関係な既定 profile の hash は不変です。実際の公開中立 v0.4.2 → v0.4.3 通常更新も完了しました（`81c82d837b65fb03584bfb1e668119c3`、`standalone_replace`）。同じ profile/Herdr 0 ready・ルベリア UI を確認し、Herdr 再導入はありません。Check は `last_update_check` のみ変更し、Check 後は全 config bytes と旧 A 証拠87個が更新を通じ不変でした。最終公開 v0.3.9 A/B や v0.4.4 更新の証明ではありません。


## 動作要件

Apple Silicon Mac、macOS 13 以降。ソース構築には Rust/Cargo、Bun、Node.js/npm、Xcode Command Line Tools が必要ですが、プリビルドには不要です。ad-hoc 署名で、公証されていません。Herdr スナップショットは互換ローカルサーバー（0.9.0+）、返信は別途 `agent.prompt` が必要です。任意の client-attach 自動起動には公式 Herdr 0.9.3 に[連携所有のパッチとホスト手順](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)を適用するか、実際にフック対応のホストが必要です。標準 0.9.0/0.9.3 にはフックがなく、プラグイン導入はホストをパッチしません。

## インストールと任意の連携

**現行の安定版候補の所有者方針:** 新規 Standalone と通常更新は実 OS home の `.local/share/desktop-pet/libexec/DesktopPet.app` を使い、`HOME`/XDG に依存しません。元の `Applications/DesktopPet.app` は旧 helper の完了確認、別途承認した移動、復旧のためだけに認識し、恒久的な同じ場所での更新先にはしません。任意の home/custom/system パスや symlink の引継ぎは禁止です。Brew/source 所有者と過去の凍結ノートは変更しません。

### 既存 Applications 所有者: 明示承認した一回限りの復旧

本物の beta.7 helper は新しい安定版を選べますが、実機で別の不変 helper の不具合が確認されました。応答後に閉じたソケットの Darwin peer PID を再取得し、導入前に `cannot authenticate control peer PID` で失敗します。アプリ内確認だけでは配布済み helper を修正できません。[0.4.14 修正](releases/v0.4.14.md)は認証を直し、明示承認した運用者復旧を提供します。旧 updater の反復や以前のアプリ内経路の成功を前提にしないでください。

承認済みの運用者復旧には [v0.4.16 以降](releases/v0.4.16.md)を使用してください。SDK 内部デフォルト pack 検証の認証と、隔離 helper への信頼できる HOME の設定を含み、任意の pack コマンドや別 profile は許可しません。Tern の終了や導入保護の迂回で遮断を解消しないでください。

認証した元の config/state、キャラクター、設定、ユーザーデータを保持します。既存 Tern SDK が native 処理の確定後に保存済み executable だけを変更し、native は KV を上書きしません。既定値の推測、profile 結合、Configure/Connect、Tern 再起動、plugin reload で失敗を隠さないでください。署名済み原本と証拠は private retention に保持します。

**Pet の未保存編集を先に保存してください。** 運用者復旧は原本・profile の backup と、Pet だけの正常停止、その後の別途 Start への明示承認が必要です。停止済みソースの導入は Stop ACK、updater parent、再起動 token を捏造しません。後のユーザー Stop や scope 変更は自動再起動を禁止します。確定済み Stable parent の同一バージョン native 移動とは別の承認です。

native 導入・準備と窓ごとの fresh Tern 確認は別です。未接続・非互換 consumer は連携待ちであり native 導入失敗ではありません。公開やソース検査だけで実ユーザー復旧を主張せず、Tern 初期化や未確定 journal 削除なしに実際の観測結果を報告します。

### 過去の固定インストール例

**過去の v0.4.4 新規 Brew 導入証明:** 中立 v0.4.4 formula は tap `2aa9a4be9517e0232a3f2b8e2b3c748045c69586` に公開済みです。適格な Brew v0.4.4 新規導入は通過しました（74ファイル / 44.3 MB、内外の再署名5個）。認証済み検査で Homebrew 派生 image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312` を確認しました。導入・検査は中立 config/state・standalone app を作成せず、デーモンも起動しませんでした。明示的な手動 **Start** の後だけ実際の v0.4.4 UI/control ready、PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470` に達しました。ルベリアと実際の Settings は **Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0** を表示し、その後の明示 Hidden は同じ既定 instance を保持しました。新規導入の証拠であり、v0.4.4 通常更新や managed 移行の完了証明ではありません。導入コマンド:

```sh
brew install hanbong5938/tap/desktop-pet
desktop-pet --version
desktop-pet start
desktop-pet status
```

プリビルド・ソース導入は Herdr 連携ではなく **core** チェックアウトで実行します。既定はプリビルドのみで、取得・検証失敗からソースビルドへ黙って切り替えません。`--source` は現在の core だけを構築し、その `dist/DesktopPet.app` を使用します。

**Historical pinned v0.4.7 installation examples, not latest-version instructions:** the commands below deliberately select immutable older assets. Core v0.4.11 is public but failed historical repair; consumer metadata remains v0.4.8 pending actual verified v0.4.12 / v0.3.16 publication. Never substitute unpublished assets.

```sh
git clone https://github.com/hanbong5938/desktop-pet.git
cd desktop-pet
# Published immutable v0.4.7 installer and pinned assets:
git checkout --detach v0.4.7
bash scripts/install.sh --prebuilt
# 同じ公開 v0.4.7 タグでソース導入を選ぶ場合:
# bash scripts/install.sh --source
# Replace with the installer-reported executable and your actual profile paths:
/absolute/reported/desktop-pet --config-dir /absolute/dev/config --state-dir /absolute/dev/state start
```

不変 v0.4.7 の既定／`--prebuilt` は正確な公開固定アセットを使用します（過去の v0.4.0／v0.4.1 は各 pin を保持）。取得／検証失敗をソース構築で隠しません。明示 `--source` は実際の公開 v0.4.7 タグを含む選択 core checkout を構築します。 standalone prebuilt は **実 OS アカウント home** の `Applications/DesktopPet.app`、承認された source は core の `dist/DesktopPet.app` を使います。admission は検証済み健全な既存所有者を再利用し得るため、推測した dist パスではなく導入器が示す実行ファイルを使います。共有 locator はアカウント home の `.local/share/desktop-pet/installation.json` に原子的に保存し、探索証拠であって権限ではありません。不正な明示パス／locator は fallback せず、実行ファイル、署名、capability、origin を再検証します。Brew は実際の formula／prefix を所有し、生存／予約中の導入を密かに引き継ぎません。

必要な連携だけを導入します。Herdr は薄い consumer です。Tern はネイティブアプリを取得・構築・インストール・自動起動しません。プラグインの更新・削除は共有アプリ、プロファイル、キャラクターを削除しません。通常のアプリ更新で Herdr プラグインを再導入しません。

```sh
herdr plugin install hanbong5938/herdr-desktop-pet
tern plugin install https://github.com/hanbong5938/tern-desktop-pet
```

### Tern を閉じずに Standalone を更新

現在のソースは固定 prebuilt 導入器に Pet-only transaction を追加します。公開リリースの再配布や実際の Tern acceptance 完了を示すものではありません。低い版の**同じ Standalone 所有者**を更新するとき、native dialog が正確な版・root・profile を示し、**Desktop Pet だけの再起動**を承認します。Tern、ウィンドウ、ターミナル、agent の作業は維持し、plugin reload や Configure/KV の書き換えを要求しません。**Pet の編集内容は先に保存してください。** 検証済み公開 beta.1 の bootstrap は draft-aware Prepare ではなく通常の Quit です。Cancel、承認画面の失敗、異なる active native profile の競合は停止前に拒否します。同じ native profile の Tern 二窓は競合ではありません。停止中の profile は導入だけで起動せず、Homebrew/Source の所有権や beta 設定も変更しません。

`installed` は検証済みディスク交換、`applied` は新しい exact-profile instance と UI/control ready の確認です。再起動失敗なら `installed:true, applied:false` が残り得ます。導入器が示す operation ID、state-dir、保持した署名 helper を保存し、その `install-status` / `install-recover` を使います。手動 Stop/再導入や通常 updater の `recover --start` と混同しないでください。status の exit 0 は観測の成功であり、導入成功ではありません。失われた Stop ACK や後続の user Stop は復旧承認で再起動権限に変わりません。旧 signed bundle と元の sidecar は private storage に保持し、自動削除しません。証拠や予約 marker を削除しないでください。[CLI 契約（英語）](cli.md#pet-only-standalone-installation-transaction-current-source-contract)。


## プロファイル・接続・移行

新規プロファイルは Auto です。Connection は登録されたプロバイダーの実際の接続を示し、インストール有無を接続と扱いません。ID は不透明な値で、権限は名前の接頭辞やパスではなく capability で判断します。Auto は同時観測で、fallback ではありません。リモートは読み取り専用、Tern は返信のみで worktree 削除は不可です。受付はエージェントの完了ではありません。

[profile・探索契約](development.md#profiles-and-installation-discovery)と[移行・保存履歴](migrations/desktop-pet.md)を参照してください。予定の移行は実際の executable/config/state を自動で引き継ぎ、同じ Tern ウィンドウを fresh rebind してから retire します。手動 Configure/Connect を追加しません。移行外の初回独立接続は明示設定できます。既存 Tern・beta・custom profile はその場で保持し、統合しません。

```sh
desktop-pet profile show --json
desktop-pet --herdr-socket /actual/path/to/herdr-api.sock start
desktop-pet preferences set --observation-host auto
```

## 操作

| 操作 | 効果 |
| --- | --- |
| メニューバーアイコンを左クリック、キーボードで起動、または AXPress | **見た目／接続／アプリ**の３タブ設定パネルを開きます。再度左クリックしても選択タブを戻さず前面に出します。キーボードフォーカスは macOS が決定します。 |
| メニューバーアイコンを右クリック／Control-クリック | パネルを隠してから、ネイティブの**設定…**、**終了**、必要なときの**キャラクターを表示して有効化**を開きます。 |
| 頭／胴をタップ、頭を往復してなでる | 反応を起こし、キャラクターをなでます。 |
| 胴／背景をドラッグ、Option を押してキャラクターウィンドウをドラッグ、右下のグリップ | 移動／キャラクターのサイズ変更。 |
| 吹き出し背景をドラッグ | 接続時はキャラクターを、単独時は吹き出しだけを移動。 |
| 吹き出しの縁／角をドラッグ（Option 押下も可） | 縁は１軸、角は２軸を変更。キャラクターの拡大率／フォントサイズとは独立。 |
| 吹き出し背景／カード見出しを右クリック／Control-クリック | **吹き出しを閉じる**は吹き出しだけを隠し、**自動サイズに戻す**は折りたたみ／展開のサイズ指定を解除します。テキストのコピー／ペーストなどネイティブのメニューは保持。 |
| ウィンドウ全体／アルファマスクのクリック透過 | 全体透過ではキャラクターと吹き出しに触れられません。アルファマスクでは透明なキャラクター画像のみ透過し、吹き出しは操作できます。 |

**終了**は Desktop Pet の現在の実行だけを終了します。Herdr・Tern は終了せず、自動起動設定も維持します。

**このソースチェックアウトでは：**左ボタンの捕捉が中断しても、ボタンを放した後で偽のタップ反応なしに復旧します。新しい押下には固有のドラッグ起点が付き、重複／古いイベントは起点の入れ替えや別ウィンドウの操作完了に使えません。Control-左クリックの解放は古いタップを完了しません。通常タップ、Option ドラッグ、リサイズグリップは維持します。解放コールバックを失った有効な吹き出しのリサイズは、後述する追跡と IME 完了条件のもとでプレビューを一度だけ確定します。この変更にはチェックアウトの再ビルドとインストールが必要で、インストール済みのリリースバイナリには反映されません。

折りたたみ／展開のサイズ指定は別々に保存し、接続／単独表示では共用します。開始した画面でのプレビューと確定中、反対側の縁／角は固定され、以後の接続配置はキャラクターに追従します。縁操作で変更する指定軸は１つですが、折り返しによって他軸の表示下限が増えることがあります。高さは実際の画面／アンカーで制限された幅で測り、保存に使う固有の幅下限は下げません。非対象軸だけの動きや対象軸がゼロへ戻る動きでは指定を維持します。完了時は最新の自動／手動の下限で再配置し、拡大も縮小も可能です。展開されたメッセージとカードは高さ予算を共用し、カード／返信の最小値とメッセージ１行を先に確保します。自動配置は全体高さの上限を守り、手動指定はその自動上限を使いません。収まるとスクロールバーを隠し、はみ出す場合はスクロールできます。小画面では表示だけを制限し、サイズ指定を破棄しません。

接続配置では、頭部のアンカーだけでなくキャラクターの安定境界から吹き出し本体全体を離します。**自動**は現在の側に返信／操作の最小寸法が収まる限りその側を維持し、タスク展開、返信開始、内容の再配置で側選びをやり直しません。保存値は上書きせず、その側で利用可能な範囲に表示サイズを制限できます。接続リサイズは捕捉した側の安全領域を使用し、反対の縁を固定します。どの側にも最小値が収まらなければ、キャラクターウィンドウの手前で境界内に収まる、尾なしで重なりを最小にする配置に戻します。場所を空けるためにキャラクターや保存サイズは変えません。

返信を開く際はフォーカスする前に接続配置を測り、選択行、入力欄、カーソルを見える位置に置きます。再配置してもネイティブエディタ、下書き、選択は維持します。IME 変換中の内容／配置変更はエディタの親やサイズを変えずに待機し、画面／キャラクターの形状変更でウィンドウが危険になった場合だけ、安全な位置への移動を変換終了前に許します。

操作中でない場合、吹き出しが実際のポインタ対象のときだけ再配置がリサイズカーソルを更新します。ネイティブテキスト／操作部のカーソルはそのままにし、別ウィンドウ／キャラクター上のカーソルをリセットしません。サイズ移動がゼロでも単独表示の実際の原点が変われば保存し、指定と形状が不変なら設定を書きません。返信 IME の変換中はリサイズ開始と**自動サイズに戻す**を禁止します。押下後に変換が始まればプレビューを一時停止し、解放／キャンセルで追跡を終えますが、変換が安全になり追跡が終わるまで保存／保留中の更新を延ばします。カードと下書きはそのままです。最新の内容、言語、配置、画面への収まりから一度だけ保存します。保存失敗時は以前のサイズ指定、単独表示の実際の原点、保留状態に戻し、幽霊の保存を残しません（新内容による表示下限の変化はあり得ます）。自動サイズへの復帰は位置、可視状態、表示モード、下書きを維持し、位置／色のリセットはサイズ指定を消しません。返信に未確定文字がある間はメニューバーアイコンから直接設定を開けません。変換を終えてください。Escape／外側クリックで設定パネルを閉じる動作は残ります。

## セッションの検索と並べ替え

カード名は観測されたターミナルのタイトル、名前付きタブ、ワークスペース／ディレクトリの順で決めます。欠けた名前には明示的な代替名を、重複には識別表示を付けます。ホバーでタイトル全文、作業ディレクトリ、内部 ID を確認できます。タイトル変更でも選択／スクロールを保ち、切断したカードは最後のタイトルと**オフライン**を示します。

タイトルや作業ディレクトリが同じでも、同じ会話とは限りません。**ローカル**／**リモート**は観測元の場所、接続中／**オフライン**は利用可能かどうかを示します。ローカルの観測元もオフラインになり、保持されたカードはライブ接続の証拠ではありません。

カードの選択は Desktop Pet（Core）の返信先を決めるもので、Tern のアクティブな pane を切り替えません。キャラクターは観測中のすべての観測元の状態を集計するため、現在フォーカス中の pane の状態と異なる場合があります。

空の一覧は接続状態とセッション数を区別します。**セッションなし**は対象の接続元から正常な空のスナップショットを受信した場合、**接続切断 · 接続設定を確認してください**は対象の接続元がオフラインの場合、**接続なし · 接続設定を確認してください**は対象の接続元がない場合です。接続元が期限切れで削除された後も最後の案内になります。**該当なし**は既存カードが検索・フィルターで隠れた場合です。作業が終了したと判断する前に、**設定 → 接続**を確認してください。

**検索**は前後空白を除き、タイトル、タブ／ワークスペース、エージェント、観測元の表示名、作業ディレクトリ全文、代替名を大文字小文字を区別せず部分一致させます。ID やソケットパスは対象外です。検索と**状態**フィルタは積集合で、全体の観測状態／反応は変えません。**並べ替え**は**安定順**（元の順番）、**名前**、**観測元**のほか、区切り線の下に独立した**実行中を優先**があります。対象の全観測元の実行中セッションを先にし、各群で選択した順に並べます。閉じたメニューとアクセシビリティ値には両方の選択を表示します。全状態での並べ替えではありません。GUI の表示はフィルタ／並べ替え**後**に最大 128 カードで、省略数を表示します。CLI `sessions list --limit` は**ページサイズ**であり総行数の上限ではありません（[CLI、英語](cli.md#sessions-and-agent-prompt)）。

初期値は検索空欄、状態**すべて**、並べ替え**安定順**、実行中優先オフ。並べ替え／実行中優先は保存成功時だけ再起動後も維持し、保存失敗時は操作を戻します。検索と状態はメモリ限りです。選択した返信がフィルタで消えると欄を畳みますが下書きは維持し、検索を消しても再び開きません。検索にフォーカスがある Enter／Command+Enter は送信しません。Escape はまず検索 IME をキャンセルし、次に文字のある検索を消し、空欄なら吹き出しを畳まず欄を抜けます。検索と返信の変換はそれぞれマーク解除まで構造的な一覧変更を待たせ、ライブ状態は更新し続けます。

## インライン返信とメッセージ配送

インライン返信は v0.1.6 で初登場し、安定版 v0.2.0 でネイティブ文字の欠けと Command-A/C/X/V のクリップボード操作を修正しました。吹き出しを展開しても最初はカードだけで、**すべて**の場合も返信欄は開きません。ローカルのエージェントカードをクリックすると１行欄が開き、別のカードを押すと元の欄を畳んで次を開きます。リモートカードは読み取り専用で欄も送信もありません。入力、ペースト、インポートした確定済み改行は空白にし、IME 未確定文字には確定まで手を加えません。**Enter**、**Command+Enter**、**送信**で提出しますが変換中は提出しません。**Escape** はまず変換を取り消し、それ以外は返信欄だけを畳みます。外側クリックも変換中以外は畳み、吹き出し内の別のクリックでは畳みません。いずれも吹き出し自体は閉じません。セッションの下書きは欄／吹き出しを畳んでもメモリに残り、再起動後は残りません。成功時はまだ一致している返信だけを消して畳みます。送信中の編集や選択変更は新しい下書き／開いた欄を維持し、失敗や配送不確実でも下書きと返信状態を維持します。

変換中は選択、フィルタ、構造的な行交換を待たせ、マーク中の文字を失いません。マーク解除後、最新のフィルタとまだ表示できる有効な選択だけを適用します。確定文字は元のセッションと接続世代に所属します。ライブ状態／送信可能かどうかは更新します。欄は視認できる枠とパレット由来の不透明背景を持ち、小型のネイティブ送信ボタンは可否に連動し、テーマ変更では下書き／未確定文字を維持します。

Herdr カードの配送は選択した**ローカル** Herdr 観測元の `agent.prompt` API **のみ**を使い、グローバル CLI、シェル、SSH、生の pane 入力には頼りません。ローカル観測の除外、オフライン元、古い／未準備セッション、非対応サーバーでは送信できません。`unsupported_method` は通知し、迂回しません。保持されたリモートカードも送信不可です。承認／質問は実際のターミナルで処理します。`agent.prompt` の検証は Herdr 0.9.2 で行われ、本地観測の最低 0.9.0 は実際のサーバーが対応する証明ではありません。提出 ACK は**エージェントの完了ではありません**。自動再試行はなく、不確実なら確認してから手動で再送します。API は現在の pane 使用者に宛て、期待セッションの原子的条件を持たないため、瞬時の使用者／セッション入れ替わりは残ります。自動操作は[CLI のセッション識別と配送（英語）](cli.md#sessions-and-agent-prompt)を参照してください。GUI の代替経路ではありません。

Tern の返信は登録アダプターが再検証した **１回限りの `agents:ask`** を使い、Herdr fallback、shell、PTY 入力を使いません。`prompt_accepted` / `accepted_may_be_queued` は SDK の受付だけを示し、モデルへの配送や完了ではありません。提示済み／不確実な ask を再送しません。ローカルのウィンドウ／グループ化したホスト一覧はアダプター承認の locality を維持し、遠隔カードは読み取り専用、Tern に worktree 削除権限はありません。ウィンドウ元は５秒で期限切れとなり Offline になりますが Auto／アダプターなしのアプリは継続します。一覧取得は４つの協調的 zero-delay callback に分け、ask の結果後に新しい交換を直ちに始めます。単一 SDK 呼出しや OS の先取りは予算を超え得るため、遅延／安定性の保証ではありません。

## 表示とメニューバーからの復旧

キャラクターの `show`／`hide`／`toggle` と**吹き出し → 吹き出しを表示**の `show_bubble`／`hide_bubble` は独立です。両方オンなら接続し、キャラクターだけなら吹き出しを隠し、吹き出しだけならカード／返信付きの尾のない移動可能な単独ウィンドウになります。両方オフでもアプリ／Herdr セッションは動作します。**吹き出しを閉じる**は保存された可視状態のみ変更し、セッションやメモリ中の下書きを削除しません。コンテキストメニューをキャンセルしてもカードの選択／下書きは維持します。返信 IME の変換中は**設定…**／**吹き出しを閉じる**が無効で、実行時にも再確認します。変換終了後にメニューを開き直してください。キャラクター／単独吹き出しのコンテキスト設定または見えているメニューバーアイコンから**吹き出し → 吹き出しを表示**を開きます。単独吹き出しのコンテキストメニューには**キャラクターを表示**もあります。

キャラクターを隠しても保存済みの吹き出し可視状態は変わりません。独立位置の保存がない最初の切り離しでは、単なる計算位置ではなく、**実際に適用した接続吹き出し本体**の位置を取ります。最初からキャラクターが隠れていれば通常の接続配置が代替となります。その原点はキャラクター位置とは別に保存し、以後マーク中の文字があっても切り離し時に利用します。背景ドラッグは単独吹き出しだけを動かし、再表示で接続、再び隠せば保存位置へ戻します。折りたたみ／展開、返信、再測定でも画面への収まりを除き位置を維持し、画面の取り外し／配置変更で画面外の位置は復旧します。選択、表示／返信状態、下書きはこの実行中の表示切り替えで維持し、再起動後は残りません。単独時の接続配置の変更は、単独の原点を動かさず、再接続時に反映します。`reset` は旧単独原点を消し、必要時に新たに適用された接続配置から採り直します。キャラクター座標をコピーしたり可視状態を変えたりしません。返信のマーク中の文字は表示／非表示／配置／reset の原点確定をマーク解除まで待たせ、新しい reset が古い原点より優先します。エディタのコールバックでマーク文字、確定下書き、選択の維持を観測していますが、実際の IME／フォーカス認証ではありません。

**アプリ → メニューバー設定… → 表示条件**は**常に表示**（安定版 v0.2.0 以降の既定）か**復旧が必要なときのみ**を選べ、ウィンドウ表示とは独立です。復旧専用では、**両ウィンドウが非表示**または**ウィンドウ全体のクリック透過**時に表示します。操作可能なキャラクターのみ、単独吹き出しのみ、両方表示、アルファ透過だけ（両方非表示でない場合）では出ません。常時表示ならいずれでも出ます。終了中はどちらもアイコンを出さず、アルファポインタの監視だけでは復旧表示しません。右クリック／Control-クリック時の**キャラクターを表示して有効化**は復旧が必要なときだけです。キャラクターを表示し、全体透過を解除し、吹き出しの可視状態とアルファ透過を維持し、可視吹き出しを接続します。メニューを閉じると復旧専用アイコンは消えます。CLI `show` はキャラクターだけを表示し、吹き出しは表示しません。旧 `bf23ced` ローカルプレビューの条件付きアイコンは履歴であり、現在のプロファイルに継承される設定ではありません（[プレビュー記録、英語](migrations/unreleased.md)）。

`preferences.json` の `menu_bar_mode` は `always` または `recovery_only` で、省略時は旧条件付きプレビューのプロファイルも常時表示です。保存成功なら即座に反映し、再起動／別の保存でも維持し、未知のフィールドを残します。保存失敗なら旧モード／アイコンを維持します。モード変更は表示、透過、位置、下書き、ライフサイクルを変えません。

**設定 → メニューバーアイコン → 画像を選択…**は**静止 PNG**（符号化サイズ ≤4 MiB、復号後 ≤100 万画素）を取り込みます。アニメーション、破損、欠損、完全透明を拒否します。元の色を維持して 18 pt にアスペクト比を合わせ、1×／2× Retina の表現を用意します。設定ディレクトリの `menu-bar-icons/` に管理コピーを作り、任意の `preferences.json` の `menu_bar_icon.asset` で参照するので、元ファイルは移動できます。`preferences.json`、`lifecycle.json`、`characters/` とともにこのディレクトリもバックアップしてください。指定がなければ標準の足跡を表示します。取り込み／保存失敗は以前の選択を維持し、再起動時の管理画像欠損／破損はメタデータを維持しながら**エラーを示して**標準画像へ戻ります。**標準アイコンに戻す**は画像だけを変え、モード、ウィンドウ、透過、形状、下書き、ライフサイクルには触れません。ネイティブ復号の前に**全 PNG チャンクの CRC**（補助チャンクと IEND も）、IDAT zlib Adler-32、色／インターレース各走査の正確な展開行長を検査します。コピーは既存ファイルを置換せず原子的に公開します。過去に中断した私有の一時ハードリンクは、自分が所有する私有の同 inode 別名と確認できる場合に限り、**同一で有効な PNG**を明示的に再インポートして修復できます。起動時の全体走査や任意のリンク、別ファイル、壊れた PNG の修復はしません。ベータ版の履歴証拠は[別記（英語）](releases/unreleased.md)で、実際のピッカー／VoiceOver 認証ではありません。

## ワークツリーの削除

展開した吹き出しの**カード見出し**を右クリック／Control-クリックすると、別のカードが選択中でもそのカードが対象です。**背景**で開くと*メニューを開いた時点*の選択カードを対象とします。対象がなければ**ワークツリーを削除…**はありません。メニューは対象を明示し、入力欄などのネイティブメニューは維持します。**吹き出しを閉じる**は隠すだけです。

確認画面はリポジトリ、**正確なチェックアウトパス**、そのカードだけでなく**ワークスペース全体**のタブ／pane 数を示します。削除すると全タブ／pane が閉じ、エージェント／プロセスが終了し、結びついたチェックアウトを**無視対象ファイル／ビルド出力も含めて**消去します。主リポジトリや Git ブランチは消さず、ゴミ箱／取り消しもありません。**キャンセル**が先頭で既定です。Return、テンキー Enter、Escape はキャンセルし、**ワークツリーを削除**は別の第２操作です。配置後のネイティブ警告でキャンセルを既定にできなければ削除を中止します。所有する使い捨てのチェックアウトと影響を受けるファイル／プロセスを確認してください。

対象は稼働し整合するローカル観測元と、一意で有効なリンク済みワークツリーメタデータを持つ場合だけです。主ルート、リモート／保持済み／オフラインカード、欠落／不正なメタデータ、非対応の旧サーバーは対象外。`worktree.remove` が非対応なら Git／shell／SSH に迂回して強制削除せずエラーにします。IME 中は削除できず、送信前にも再確認します。一度に１件の操作が対象を固定し、確認後に新しいスナップショットで再検証し、`force: false` の `worktree.remove` を**１回だけ**送り、再試行や楽観的な行削除はしません。追跡済み／未追跡ファイルの変更やロックは拒否要因ですが、**無視対象ファイルは削除され得ます**。表示行は５秒の watcher 更新に従い、選択／行／ウィンドウが変わっても結果表示は残ります。不確実な書き込みは手動再試行**前に**サーバーとワークツリーを独立して確認します。リクエストの `workspace_id` に原子的な期待チェックアウト／世代 CAS はなく、クライアントの再検証でも再起動／再結合の競合は残ります。[CLI の検査／トークン仕様（英語）](cli.md#worktree-removal)は検査済み対象の 60 秒の１回用トークンを追加し、上流 ACK、後から確認した pane＋セッションの不在、実ファイルシステムの証明を区別します。稼働中のチェックアウトで試さないでください。過去のベータ版観測は安定版の実入力認証ではありません（[記録、英語](releases/unreleased.md)）。

対象は登録元の local／worktree 削除 capability も必要です。提供者名やローカル風のパスでは権限を得られず、Tern に削除 capability はありません。

## 吹き出しとキャラクターの設定

**現在のソース構成（公開済みという意味ではありません）:** 352×540 pt の設定パネルは**見た目／接続／アプリ**の３タブで、同じ画面内制限とタブ別スクロール位置を使います。見た目には現在のキャラクター／候補／操作、ブラウザ、表示、サイズ、吹き出しの位置／テーマ、位置リセットがあります。接続は選択先と登録プロバイダーの実際の接続数を表示し、照会失敗を接続成功に読み替えません。アプリには言語、**連携からの自動起動を許可**、更新状態／操作、ベータ有効表示があります。連携の自動起動は macOS ログイン時の起動設定ではありません。

７つのボタンは１つの非モーダル・サイズ可変ウィンドウの該当ページを直接開きます。**見た目の詳細設定**は状態表示、クリック透過、５色、セリフ編集と外観エラー、**接続設定**は自動／プロバイダー選択、接続数、提供者の設定／操作／ヘルプ、**メニューバー設定**は表示条件とアイコン、**更新設定**は自動確認とベータ方針です。**診断情報**は実行ファイル、設定／状態／ソケット、実行識別子、接続／自動起動のエラー、**更新ログ**は導入元、リリース、確認時刻、操作／移設、履歴、診断を選択可能な等幅の読み取り専用テキストで表示し、**アプリの復旧**は設定の初期化を提供します。各ページの実測内容に高さを合わせ、スクロール位置とユーザーが選んだサイズを保持します。大きなログのサイズを短い設定ページに引き継ぎません。基本パネルを閉じたり別アプリを有効にしても詳細は残り、開くだけでは更新確認／インストールしません。

**アプリの復旧 → すべての設定を初期化…**は確認が必要で、未保存編集・IME・処理中の作業・未解決の更新／移行がある間は拒否します。現在の正確なプロファイルの設定を非公開バックアップに保存し、言語・外観・セリフ変更・キャラクター選択・観測・自動起動・更新設定を既定に戻して Pet のみ再起動します。導入済みパック、元画像、起動引数のアセット、連携アプリの接続／登録、プロファイルのパス、未知の拡張メタデータ、更新履歴と予約は保持します。最後の結果とバックアップのパスを表示し、失敗を成功扱いせず、後のユーザー停止は再起動に優先します。完了・適用済み更新の履歴は予約解放を別途検証できれば自動確認／ベータ設定を妨げません。不明／実行中の更新は引き続き保護します。

更新の状態アイコンと見出しはコンパクトなカードの外側上部に表示します。メニューバー設定は言語の下、自動起動の上に分離し、更新設定と更新ログは１行に並べます。ログは既存のサイズ可変・スクロール可能なウィンドウを使い、基本パネルを伸ばしません。

更新が中断した場合は、**アプリ → アプリ更新**から復旧状態を確認できます。安全な予約整理と、検証済みアプリの明示的な再起動は別の操作です。インストール開始前の失敗は、元のアプリが同じ状態で実行中と確認できれば終了せずに整理します。別のヘルパーが処理中なら進行状況を表示し、重複実行しません。結果不明や管理プロセスの未確定状態は保護したまま表示し、ログの選択・スクロールは利用できます。整理完了は更新成功を意味しません。新しい更新には新たな確認と同意が必要で、ユーザーの終了要求を優先します。

古いヘルパーが実行中の元アプリを安全に確認できない場合は、**Desktop Petを終了して復旧…**を別操作として表示します。未保存編集・IME・保留中の操作を保護し、ネイティブ確認を承認するまで終了しません。現在の署名済みヘルパーが、元の署名済みヘルパーによる記録確認・整理を調整します。Ternやほかの作業は終了せず、インストーラーや自動再起動も実行しません。整理後の手動起動は別の同意が必要です。

手動起動には、選択された署名済み実行ファイルが `manual_start_fence_contract:1` を実装し、起動中の新しいユーザー停止要求を拒否できることも必要です。beta2 など未対応の旧アプリは予約整理だけを行い、安全でない起動や偽の更新トークンで回避せず、足りない起動機能を画面に表示します。

**カスタム**テーマの保存成功後は見た目の詳細設定を開きます。５色は**色を適用**で保存し、色のリセットも保存操作です。ページ変更、閉じて再度開く操作は保存せず、不正値、空値、大文字小文字混在の下書きを保ちます。競合時だけ**保存済みの色を読み込む**／**編集を保持して基準を更新**が現れます。前者は下書きを置換し、後者は編集を保ちます。競合は古い基準での適用を拒否しますが閉じることは妨げません。IME 変換中は閉じる／ページ変更／画像選択を保留します。更新中の凍結でも診断の選択／スクロールは可能で、変更操作だけを無効にします。

**見た目 → 吹き出しのテーマ**と**アプリ → 言語**はネイティブのドロップダウンです。**システム**は macOS の優先言語順に従い、韓国語（`ko`）、英語（`en`）、簡体字中国語（`zh-Hans`）、日本語（`ja`）を明示指定できます。日本語と簡体字中国語（`zh`、`zh-Hans`、`zh-CN`、`zh-SG`）を認識し、明示的な繁体字（`zh-Hant`、`zh-TW`、`zh-HK`、`zh-MO`）は次の対応言語へ進み、なければ英語です。保存値の綴りは不変で、テーマ／言語は即時保存します。ネイティブ追跡中の吹き出し表示／配置変更は追跡終了後に最新値を適用します。

**見た目の詳細設定 → 状態アイコンと色を表示**は既定でオン、即時反映して保存します。オフにすれば文字だけです。実行中は青、待機は琥珀、**検証済み**の成功は緑のチェック、失敗は赤。`Done`／承認された結果のないアイドルは中立で、プロバイダの成功を推測しません。一致した検証済み最終報告はアイドル／スナップショットをまたげますが、新活動、セッション識別の曖昧さ、報告喪失、切断で消えます。曖昧さが解けても同じキャッシュ報告を再受理しません。新しい観測元は空のものも含め最初の整合スナップショットまでオフラインです。サマリーの緑はカードフィルタ／表示上限に関係なく**観測中の全セッション**が検証済み成功であることを意味します。状態は文字も保持し、アクセシビリティではアイコンを装飾扱いにし、カードは重複子要素なしでタイトル／状態全文を提供します。アイコン切り替えは選択、フィルタ、下書きを維持します。アルファ透過はポインタ監視を用いるので、クリックの競合が許容できない場合はオフにしてください。

**見た目**タブは現在／候補／操作の簡潔なサマリーを保ち、候補も有意な結果もなければ空の選択／状態カードを隠します。キャラクターだけの高さ特例はなく、３タブとも同じ制限された表示領域を使います。キャラクターパック v5 と `default@0` Rubelia は変更なし（[v0.3.3 の修正、英語](releases/v0.3.3.md)）。

## セリフの編集

**見た目の詳細設定 → セリフを編集…**で別のサイズ可変ダークテーマのエディタを開きます。既定内容 760×560、最小ウィンドウ 620×480。アクティブ化せず任意のキャラクターと**原作者のリビジョン**を選び、UI 言語とは独立に韓国語（`ko`）、英語（`en`）、簡体字中国語（`zh-Hans`）、日本語（`ja`）を選べます。イベントはアイドル、実行中、待機、不明、頭タップ、胴タップ、なでる、観測したタスク完了の８つ。上部に選択、左にイベント、中央に編集と展開できる原文参照、下部に固定の状態／操作。長い名前／エラーはホバーで全文を確認できます。未保存の項目には印が付き、上書きがなければ元の文を入力欄に初期表示し、別の原文参照と比較できます。ユーザー文はその項目だけを置換し、観測数／状態遷移、反応時間、切断通知は変えません。

**セリフを保存**／**Command+S**は言語とイベント１件を、最大 **2048 UTF-8 バイト**（文字数ではない）まで保存します。空白だけなら上書きを削除し、パック／既定／状態の文へ戻り、吹き出しを隠しません。アクティブなキャラクター ID の上書きは過去のリビジョン編集中でも適用できる吹き出しの文を更新し、別キャラクターを編集してもアクティブ化しません。**この項目をリセット**は未保存の下書きを失うときに確認し、**キャラクターの全セリフをリセット…**は常に確認して**４言語すべて**の上書きを削除します。下書きはキャラクター／言語／イベントごとに独立し、この実行中の文脈／UI 言語変更や閉じて再度開く操作でも残りますが、再起動後に残るのは保存済みだけです。**Command+Z**／**Shift+Command+Z**は文脈別の取り消し／やり直し。保存失敗でも下書きと以前の確定文を維持します。

上書きはパックファイル**ではなく** `preferences.json` に入ります。内蔵／導入したキャラクターは ID で管理し、リビジョン、更新、復元をまたいで共用します。外部 `--assets` は別の正規化パス名前空間を使い、アクティブ化時に捕捉した身元は実行中にフォルダ名を変更しても編集できますが、再起動時にパスを変えると保存済み上書きは**移行されません**。管理レジストリ２つが破損しストアが読み取り専用に戻っても、現在の一覧世代で内蔵の原文を有効化／修復せず読めます。利用不能な管理リビジョンが利用可能になるわけではありません。

外部からの保存でエディタの下書きを無言で上書きしません。古いリビジョン／項目との競合では保存できません。**保存済みを再読み込み**で最新確定文を明示的に受け入れ、**下書きをリベース**で新しい基準に対する再検証用にローカル文を維持します。競合時も文、フォーカス／選択、取り消しを維持し、IME マーク中の再読み込み／リベースは解除まで延ばします。厳密な `(target, reference)` 選択、利用不能な参照、下書きは保持し、リビジョンを密かに変えません。最初の準備済みメタデータはフォーカス中でも未編集欄に反映され、メタデータ取得前の生の編集（空白と IME も）は残ります。延期した反映は解除後に確定し、その後のフォーカス／選択／取り消しを壊しません。自動ポーリングはキャッシュ済みメタデータエラーを保持し、明示的な開き直し／選択または新しい読み取りで１回再試行できます。CLI の対象全体の身元 JSON、原作者参照／世代、任意の基準ガードも同じ安全境界です（[セリフ CLI 仕様、英語](cli.md#dialogue)）。これらの競合操作は v0.2.0／v0.2.1 より後の機能で、旧版には遡及しません。

## キャラクターパック

Rubelia（`default@0`）が**唯一同梱のキャラクター**です。任意の PNG／rig と衣装パックは別サイトの[キャラクターギャラリー](https://hanbong5938.github.io/herdr-characters/)にあります。**キャラクター → キャラクターを閲覧…**から独立した、状態を維持するサイズ可変のネイティブブラウザを開きます。名前／ID／公式タグを検索し、**すべて**、**インストール済み**、**公式カタログ**で絞ります。静止ポートレートはプレビューであってアーカイブではなく、閲覧だけではダウンロードしません。通信／カタログ障害でも同梱／ローカルインストール済みの管理は可能ですが公式ダウンロードは無効です。非対応形式はダウンロードと適用ができません。公開済みで適格な公式カードの**ダウンロードして適用**は検証済みの取得、導入、アクティブ化を明示的に要求します。同じ ID の公式／導入済みパックはフィルタ間で**１カード**を共用し、重複しません。実際にアクティブな導入済みリビジョンのみ**アクティブ**と表示します。導入済みカードはローカル画像を使い、選択、詳細／診断、更新／削除、過去の復元を維持します。手動で導入した ID がカタログと一致しても出所／版の証明ではなく、暗黙に置換しません。リモートの 1024 × 1024 rig 画像は**符号化・復号後とも 4 MiB** の予算内で対応し、パック検証上限は不変です。

導入済みのカード／リビジョンを押しても**候補として待機するだけ**です。固定フッターを確認して**適用**すると選択します。**キャンセル**は未提出の候補を消します。どちらの画面を閉じても候補は残り、ブラウザを再び開くと検索／フィルタが戻ります。フォルダ／`.herdrchar` の**インポート**のみでは候補にもアクティブにもなりません。CLI `pack select ID` は最新リビジョンを即選択し、`pack restore ID --revision N` は存在する過去リビジョンを UI 適用なしで選択します。アクティブなパックを消すと Rubelia に戻ります。内蔵 `default@0` は導入／更新／削除できません。ブラウザを閉じてもダウンロードは止まらず、**ダウンロードをキャンセル**はコミット前だけ変更を止めます。

パック変更中は選択／適用／キャンセル／導入／更新／削除／復元を禁止し、読み取り専用の詳細／診断は残します。IME 変換中は古い表示カードの変更を止め、変更作業がなければキャンセル／詳細は利用可能です。選択画面／確認画面から戻る際は処理中かと一覧世代を再確認し、暗黙のリベースはしません。パックストアのロック競合で画像が一時的に読み込み中になり、実際のプレビュー失敗はエラーです。処理中→アイドル時には導入済みカードの更新／削除／復元の可否をカードや世代を置き換えず更新し、詳細は使えます。キャンセル／失敗は候補を維持します。**未送信要求の拒否**と**送信済みで結果不明**は異なります。フッターのツールチップ／診断から完全な操作 ID を読み、`pack status OPID` で確認し、無闇に再試行しないでください。適用後／新候補でも最新操作の詳細は診断に残ります。非アクティブパックの導入／更新／削除はレンダラの有効化がなくても完了し、生きているキャラクターと保存選択の不一致は完了と別に示します。導入／保存後にネイティブ有効化が失敗することもあり、別の事実として扱います。**成功して完了したパック操作のサマリーカードは隠れます**が診断は残り、候補、エラー、進行中、失敗、キャンセル、結果不明は表示します。候補なしの有意な結果は**操作**という見出しで、検証済みの**セッション**成功を隠しません。

パック CLI は位置指定 ID（select／restore に `--id` はなし）、任意の操作 ID、期待世代、有限待機／待機なしを使います。オフライン既定の変更は同期完了し、明示的な非同期／有限待機には動作中デーモンが必要で、勝手には起動しません。ACK はコミット／ネイティブ適用ではありません。有限待機は ACK 後の休止／状態／復号を含む１つの絶対期限を持ち、待機なしでも既に終結失敗なら終了コードは非ゼロです。不確実な作業の ID を記録し状態を確認してください。公式ブラウザ操作を作成する URL／カタログ CLI はありません。[パックコマンド、操作状態、安全仕様（英語）](cli.md#pack-commands-and-existing-boundaries)を参照し、別のコマンド一覧をここには載せません。制作とテンプレートは[開発文書（英語）](development.md#character-authoring)にあります。

## 観測元

現在のソースでは **設定 → 接続 → 接続設定**を開き、登録済みプロバイダーが提供する操作部を使います。Core 共通の「観察範囲」画面はありません。以下の **設定 → 観測元**、**この Mac**、**リモートのマシン**などは公開 v0.3.6 以前の Herdr 観測 UI と動作の記録であり、現在の Core に同じ操作部があるという意味ではありません。当時はリモートを Auto/Herdr で利用でき、Tern 専用では操作を隠すだけで保存済み選択を維持しました。

リモート観測は v0.1.6 で初めて提供され、安定版 v0.2.0 に簡潔な設定、再接続コマンドのコピーのみの機能、単独リモートでの watcher 起動修正が含まれます。ローカルの**この Mac**が既定でオン、**リモートのマシン**はオフです。リモート利用にはまず**対話式のターミナル**で [Herdr の保存マシン案内（英語）](https://herdr.dev/docs/0.9.2/connecting-machines/)に沿い SSH マシンを登録・認証します（例：`herdr machine add workbox`）。求められた遠隔サーバー設定も完了してください。その後リモートをオンにし、**観測するマシン**で有効な保存済みプロファイルを明示的に選びます。１プロファイルが観測するのは**１つの保存済み遠隔 Herdr セッション**で、ホスト全体ではありません。登録／attach だけでは選ばれません。ペットは表示したいデスクトップの**ローカル Mac**で起動します。互換の遠隔 Herdr サーバーにはペットアプリ／プラグインは**不要**で、ローカル CLI が選択済み遠隔プロファイルをポーリングします。

次の 30 秒終了対策と確定済み無効化による終了は **Herdr 専用モード**（登録 Herdr アダプター）の説明です。Auto／アダプターなしはこの対策なしで動作を維持します。

ローカルに健全なサーバーのないリモート専用利用では、初回起動**前に**導入済みネイティブ CLI で `desktop-pet settings set exit_with_herdr off` を設定できます。設定中に 30 秒で終了するのを避け、遠隔ポーリングが正常になれば必要に応じてオンに戻せます。この設定に遠隔プラグイン／Herdr プラグイン操作は不要です。**この Mac**をオフにするとローカルカードは除外されますが、ローカルのライフサイクルエンドポイントは登録解除されず、確定済みの無効化による終了を抑止しません。保存マシンのカタログはリモートがオフでも**５秒**ごとに更新し、選択済みで有効なプロファイルはオンのとき**５秒**ごとにポーリングします。選択は再起動／リモートオフをまたぎ、名前変更でも維持します。無効化／削除はカードの観測から除外するだけでプロセスを止めません。セッション ID が同じでもローカルと各遠隔元は別のラベル／識別子を持ちます。

行には名前、保存セッション、状態を別々に示し、長い診断は折り返され、画面はスクロールできます。最初のカタログ取得中と空が確定した状態を区別し、更新失敗時は旧一覧を保持して古い旨を警告します。リモートオフは旧成功ではなく**観測オフ**と表示します。選択なし、一時停止、検査中、観測中、使用不能、無効も区別します。不透明なプロファイル ID に紐づく操作部はポーリング／名前変更中もフォーカス／スクロールを維持し、項目減少時にスクロールを制限します。ラベル／アクセシビリティ名は翻訳され、**登録方法**／折りたたみ診断は読み取り専用。観測元スイッチ／選択はカード、件数、全体状態、完了／結果の反応を決め、状態フィルタは対象カードの表示だけを変えます。全部オフなら**不明**となり、意図的に除外した観測元の切断警告にはなりません。

５秒間隔では短い変化や短時間の完了を見逃すことがあります。切断時の最後のカードは**オフライン**のまま不明として数え、ライブではありません。再接続／再選択後の最初の新しいスナップショットは基準で、過去の完了を再生しません。認証には `herdr machine list` で ID を調べ、引用したプレースホルダを正確な ID に置き換え、**自分で**対話ターミナルから `herdr machine reconnect '<profile-id>'` を実行します。**エラー詳細を表示**は読み取り専用の診断を展開し、**再接続コマンドをコピー**は安全に引用したコマンドをコピーして結果を表示するだけで実行しません。アプリは認証情報を答えず、遠隔インストールせず、失敗した遠隔元をローカルポーリングに切り替えず、遠隔カードは読み取り専用です。ローカル観測は Herdr ≥0.9.0 が必要。遠隔には保存マシン API 転送対応の**ローカル CLI**（`herdr --machine <profile-id> api snapshot`、[Herdr 0.9.2 CLI、英語](https://herdr.dev/docs/0.9.2/cli-reference/#saved-ssh-machines)）と互換の動作中遠隔サーバーが必要です。CLI の欠如／非互換は設定に示し、ローカル最低版を引き上げません。`client.attached` 自動起動には**別途パッチ適用したホスト**が必要で、0.9.3 の manifest 最低条件だけでは足りません（[ホスト連携、英語](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)）。

## ライフサイクルとプラグイン操作

Core は共通 lifecycle 保存と `auto_start`（既定 on）を所有し、追加キーは登録連携が所有します。Herdr 所有者は **`lifecycle.json` が存在しない場合だけ**、最初の lifecycle 書込みで共有 profile writer 内に `exit_with_herdr:true` を原子的に初期化します。既存ファイルにキーがなければ false、明示的 false も維持します。旧真偽値 `enabled` は false を含め `auto_start` に移行します。端点、起動メタデータ、他所有者のフィールドは保持し、`preferences.json` ではなく `lifecycle.json` に保存します。不正 JSON／値は既定化せずエラー、保存失敗は既存ポリシーを維持します。Auto／アダプターなしは Herdr がなくても動作を維持し、以下の Herdr 端点／切断規則は Herdr 専用選択だけに適用します。

自動起動オンならサーバー起動または**成功したシェル／ターミナルクライアントの attach** が不在のペットを起動します。オフの場合、自動 `ensure` は不在なら正常スキップと報告し、既存のペットにエンドポイントを登録することはあっても、隠したペットを表示しません。手動 `start` はオフでも動作し設定を変えません。`stop`／ネイティブの**終了**は起動待機中も含めその実行だけを終え、`restart` は手動起動で設定／位置／拡大率を維持します。自動起動がオンのままなら次の対象イベントで再起動するため、避けたい場合はオフにしてください。Herdr プラグインの無効化／リンク解除は設定ではなく**別のホスト操作**です。標準の Herdr 0.9.0／0.9.3 には `client.attached` の実装が**なく**、公式 0.9.3 に付属パッチを適用するか、実際にフック対応の将来版が必要です。未知フックの link 警告を成功と見なさず、[ホスト連携（英語）](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)に従ってください。

**新しい**ローカルエンドポイントで有効な `plugin_list` に `desktop-pet` がなければ、未インストールであって**無効化ではありません**。ローカルスナップショット／選択した遠隔元は観測できます。明示的 `enabled: false` はそのエンドポイントを外します。このデーモンがあるエンドポイントで有効**または無効**の登録を一度観測した後に消えた場合はリンク解除として扱います。記憶は今回の検査／再接続をまたぎますが新デーモンではリセットします。**登録済みのローカルエンドポイントすべて**が明示的に無効化／解除されたと確定すれば、遠隔が健全または `exit_with_herdr` オフでも**即時**終了します。最初から登録がないものは対象外です。それ以外で終了スイッチがオンなら、**最後の健全なサーバー接続**を失う（初めから接続できない場合も含む）と**30 秒**の再接続猶予があります。クライアント detach はサーバー喪失ではなく、古いエンドポイント／失敗した再試行は猶予を延ばしません。スイッチをオフにするとカウントダウンを取消し、切断中にオンにすると新たに 30 秒を開始します。別の健全な接続があれば継続し、終了オフの手動ペットは Herdr なしで動作できます。自動起動変更は現在のペットを停止／表示せず、`show`／`hide`／`toggle` は表示だけを変えます。

稼働して有効な Herdr ホストには `herdr plugin action invoke status --plugin desktop-pet` の形式を使い、`settings`、`restart`、`stop` も同じ action-invoke 形式です。**現在のソース:** 引数なしのネイティブ `desktop-pet settings` と既存の Herdr プラグイン Settings 操作は、独立したライフサイクル専用ウィンドウではなく、**同じ統合設定タブ**を開きます。停止中なら明示的な `settings` は選択プロファイルの保存済みホストで起動し、キャラクター／吹き出しの非表示状態を保持します。ソースの停止中 `settings` は `--assets PATH` も指定できます。同じ設定／状態パスを使ってください。ネイティブ操作に Herdr ホストは不要ですが、プラグイン操作には必要です。**公開 v0.3.6** は停止中にも独立したライフサイクル専用設定ウィンドウを開く旧動作のままです。

ネイティブ `settings get`、`settings set auto_start off`、`settings set exit_with_herdr on` のライフサイクル専用 CLI 規約は変わりません。get／set は構造化 JSON を返し、set は core `auto_start` または登録アダプター所有キーと正確な `on|off` のみ受付け、ペットを起動せず、不正入力／保存失敗で他の設定を初期化しません。`status` は両方を表示します。ほかの action ID はライフサイクル `start`、`ensure`、`stop`、`restart`、`status`、`settings`、表示 `show`、`hide`、`toggle`、`passthrough`、`alpha_passthrough`、吹き出し `show_bubble`、`hide_bubble`、`bubble_above|below|left|right|auto`、配置 `reset`、`bigger`、`smaller`。[ネイティブ自動操作仕様（英語）](cli.md#english)は命令操作、絶対的な表示／設定値、私有ソケット、操作系統ごとの状態復旧を区別します。公開 `orchestrate`／HTTP サービスはありません。

## アプリ内更新

**既存所有者の復旧:** [明示承認した一回限りの復旧](#既存-applications-所有者-明示承認した一回限りの復旧)を確認してください。canonical private への復旧後はそこで通常更新します。beta opt-in、安定版限定、Homebrew 制限は不変です。

アプリ内更新が利用できなくても既存の設定とキャラクターを削除しないでください。**公式のインストール手順を開く**は検証済みリリースの公式ページを開き、説明なしのアーカイブ取得やインストーラの自動実行はしません。実際の導入方式に合う手順を使い、古いインストーラや推測した既定パスを使わないでください。Tern 制御エラーは `alpha_passthrough` スキーマエラーではなく実際のバックエンド診断を表示し、通常の状態／エラー応答を fresh binding の証明には使いません。

**設定 → アプリ更新**には独立した自動チェック（既定オン）、**ベータ更新を受け取る**（既定オフ）、**今すぐ確認**、現在版／インストール元、ポリシー別の最後の成功確認、インストール元に応じた明示的な操作があります。安定版のみなら公式の安定リリース、ベータを含めるなら対象の安定／ベータの最高 SemVer を選び、同じ基底版の正式版がベータより優先です。ベータ解除でダウングレードしません。開いている間の自動確認は１日最大１回、安定のみ／ベータ含む確認には別々の時刻があります。ベータ設定変更で以前の候補は失効し、自動確認オンの場合だけ新方針で確認します。設定や確認だけではインストール／再起動、Brew メタデータ更新、Git 変更は行わず、別のネイティブの同意が必要です。 確認は lifecycle・導入・稼働 image・選択 profile を変更しませんが、成功時は `preferences.json` のポリシー別 timestamp キャッシュ（`last_update_check` / `last_beta_update_check`）を書き込めるため、バイト単位の読み取り専用ではありません。

- **Standalone:** 固定中立 release を検証・stage し、native installation admission の下でアカウント home のアプリを原子的に置換します。共有 locator は導入の探索根拠で、host manifest/socket 親から推測しません。検証済み所有者を再利用でき、live profile/予約は危険な置換を阻止します。通常中立更新は Herdr plugin manager を呼ばず、旧所有者 retirement は最初に承認した[移行経路](migrations/desktop-pet.md)だけに属します。
- **Homebrew：**取得した管理ツール／prefix と承認済み安定版 formula／版を維持します。物理置換範囲は prefix 全体ではなく、各 keg にまたがる formula の **Cellar サブツリー**です。この formula でベータを導入せず、自動チャネル変換や別所有者の引き継ぎもしません。確認だけならキャッシュ済みメタデータを読みます。
- **ローカルチェックアウト：**現在のディスク上のファイルを明示的に再ビルドし、Git pull／reset／参照変更はしません。再ビルドした実行ファイルが同一でも、置換したインスタンスが本当に準備完了なら構いません。
- **検証済み外部インストール：**署名済みで protocol-4 互換の導入済みパッケージはインストーラを再実行せず**導入済み更新を適用**できます。安定版のみの方針でも明示的に受け入れた適格な導入済みベータは対象です。手動／不明の出所を Herdr／Homebrew と密かに再分類しません。

同意と Prepare は未保存の下書き、IME、処理中の仕事、ジェスチャーを再確認し、**元のインスタンスが確実に稼働している間**に変更操作を固定します。AX／内部コールバック経由のセリフの取り消し／やり直し、ブラウザ検索／フィルタ／ローカルフォルダ操作も含め、安全なキャンセル／整合処理でネイティブ操作状態と履歴を戻します。ヘルパー所有権の **ACK はインストール、Stop、Quit ではありません**。受理済み Prepare の後、対応する最後の表示済み／準備済み ACK が届いて初めて同意を移譲し、対応する予約済み Stop への承認返信が完全に配送されて初めて更新用停止を確定します。遅い準備は作業スレッドで進みヘルパー UI は応答し、ネイティブの 30 秒期限は待機を意味してキャンセルではありません。返信失敗は元を解凍しインストーラは起動せず、通常の利用者 Stop／Quit は更新用再起動に優先します。**ヘルパー起動前**と証明できる失敗なら修復後の新たな確認で再試行でき、起動済み／所有権不明なら証拠に基づく復旧まで阻止します。

独立した**署名済みヘルパー**、選択済み設定／状態、独自アセットは物理的に変更されるインストールの外側に置きます。同じインストールを使う別の生きたプロファイルは置換を阻止します。ヘルパーは選択済みプロファイル／バックエンド／アセットを維持し、新インスタンスが実際に準備完了か検証し、**導入済み**と**適用済み**を区別します。適用済みは公開最新版という意味ではありません。候補起動には現在のディスクから復元した pin ではなく、正確に永続化された**イメージと導入元**の組が必要です。互換の部分マーカーは管理ツールを再実行せず修復可能ですが、不完全な pin、ドリフト、管理ツール実行の不確実性は阻止したままです。独立検証済み Original は永続化された更新失敗の目的でのみ復元でき、アップグレードと報告しません。利用者 Stop は復活を禁じますが、十分に安全な停止が証明された取引の整合と解放は許します。証明された **NoSpawn** 中断で元インスタンスが正確に準備完了なら稼働／解凍を維持し、後の新しい成功確認では履歴を消さず別の計画を提示できます。管理ツールを自動で再実行したり記録を黙って削除したりしません。オフライン `update-status` は保存記録を報告するだけでライブプロセス／署名の証明ではありません。検証済み私有ヘルパーの `status` と明示的なリース付き `recover` を[更新状態／復旧仕様（英語）](cli.md#app-update-status-and-recovery)に従って使います。

旧 protocol-1/2/3 証拠は保存・阻止したまま、一致する保持済み署名 helper だけで照会・復旧し v4 に変換しません。`updates-v4/` と実 OS アカウント home の `.desktop-pet-updates` は旧 `updates/`/`.herdr-updates` と共存します。lock 順は old global → new global → ソート installation roots、legacy stop-generation も確認します。**過去の** bridge A→neutral B は別同意と検証済み v4 `migrate-legacy --plan ABS` が必要でした。予定の通常経路は最初の承認で同一 profile Commit・実 consumer rebind・native receipt、その後の承認済み retirement を自動実行します。文書・候補 package は公開や受入れの証拠ではありません。[移行・復旧](migrations/desktop-pet.md)。

## 設定とトラブルシューティング

- プロファイル優先順位は **明示的 `--config-dir`/`--state-dir` > `DESKTOP_PET_CONFIG_DIR`/`DESKTOP_PET_STATE_DIR` > default-profile record > 中立 XDG 既定値**（未指定なら `~/.config/desktop-pet`、`~/.local/state/desktop-pet`）です。core は `HERDR_PLUGIN_*` を暗黙使用せず、Herdr wrapper だけがホスト注入を明示引数に変換します。config は lifecycle/preferences、characters、menu-bar-icons、state は lock、私有 `control.sock`、daemon log、更新証拠を保持します。実際の両パスを合わせてバックアップします。`--herdr-socket ABS` は登録 Herdr アダプターの backend オプションで制御 socket ではなく、アダプターなしで core は正規化／登録しません。[正確なプロファイル・探索契約](development.md#profiles-and-installation-discovery)。
- Herdr 専用モード（登録 Herdr アダプター）で健全なサーバーがないまま約 30 秒で消えた場合は `settings get` と `status` を確認し、同じ設定／状態プロファイルで再起動前に `exit_with_herdr off` を設定します。ウィンドウ非表示とデーモン終了を区別してください。Herdr 専用で登録済みの全ローカルエンドポイントの無効化／リンク解除による即時終了は、このスイッチでは防げません。Auto/Tern／アダプターなしはこの Herdr 終了規則を使いません。
- ウィンドウ非表示や全体クリック透過は起動失敗を意味しません。前述のメニューバー復旧を使います。`plugin link` 後に実行されなければ `client.attached` 警告を調べ、手動 `start` を呼び、デーモンの `status`／ログを確認します。`auto_start` オフは自動起動を意図してスキップします。稼働中のホストをリンクするだけでは起動しません。attach 用ホストパッチの分離配置と実際のフック確認は[ホスト連携（英語）](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)。
- `Herdr watchers are shutting down` なら、継承した `HERDR_SOCKET_PATH`／`HERDR_CLIENT_SOCKET_PATH`、`PATH` 上の実際の CLI／版、ソケットのサーバーを確認します。**別のサーバー**に対する `herdr plugin list` はデーモンのエンドポイントの診断になりません。同じ CLI／ソケット／セッションを対象にし、遠隔エラーなら保存マシン転送／版／選択も確認します。パッチ済みホストは自動起動を可能にしますが、全サーバーにこのプラグインを導入しません。
- 遠隔のエラーは[観測元](#観測元)のプロファイル状態、対話認証／再接続、対応する転送 CLI を確認します。GUI は遠隔に SSH してローカル画面を期待せず、ローカル Mac で起動します。`agent.prompt` の制限と遠隔の読み取り専用は[上記](#インライン返信とメッセージ配送)を参照してください。
- 不変 v0.4.7 の既定／`--prebuilt` は正確な公開固定アセットを使用します（過去の v0.4.0／v0.4.1 は各 pin を保持）。取得／検証失敗をソース構築で隠しません。明示 `--source` は実際の公開 v0.4.7 タグを含む選択 core checkout を構築します。 **Core v0.4.7:** [Desktop Pet v0.4.7](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.7), release `413584934`, source `52514da5115f09aac22de5429c08f7acdd993258` (2026-10-15T08:39:17Z). Six anonymous assets, strict/deep signatures, protocol 4 / observation 2 / API 1 and exact 68-file closure passed. Current observed acceptance: [v0.4.7 ledger](releases/v0.4.7.md). Pages is not yet observed; Wiki publication lacks dedicated `WIKI_PUBLISH_TOKEN`.
- プロバイダ連携は Herdr 上流側の責任です。上流ライフサイクルフック導入後、既存のプロバイダセッションを再起動／再読み込みしてください。ペットは観測状態を表示し、プロバイダの結果を推測しません。版ごとの動作は[リリース履歴（英語）](releases/README.md)、ホストの詳細は[ホスト連携（英語）](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)を参照してください。
