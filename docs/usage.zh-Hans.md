# Desktop Pet — 使用指南

独立的原生 macOS 桌面伙伴。露贝莉娅回应触摸，显示 Herdr、Tern 等已注册提供商的会话，并在来源允许时发送回复。无需宿主也能运行。

[English](../readme.md) · [한국어](../readme.ko.md) · [日本語](../readme.ja.md) · [简体中文](../readme.zh-Hans.md)

## 当前公开状态与即将采用的单次批准路径

**公开依据：**在后继版本实际完成资格确认并发布之前，当前公开版仍为 Core v0.4.8。Core v0.4.9 / bridge v0.3.13 / Tern v0.2.1 是即将采用的版本，尚未完成资格确认，也未公开。以下契约不代表安装、发布或运行验收证据。

**即将采用的常规路径：**一次原始批准 → 精确同一 profile 的 `migrationReady` / Commit → 交付并加载更新的 Tern consumer → SDK optimistic identity guard 与 atomic canonical save → 实际同一 Tern 窗口中的 fresh binding → 自动 native handover receipt → 已批准的旧所有者 retirement。**真实重连和 receipt 必须早于 retirement**。保存路径或 ACK 不代表重连；SDK guard 不是真正的 cross-VM transactional CAS。常规路径不额外要求 B/source review/Migrate 批准，也不要求手动 Configure → Save → Connect / Resume。多个 profile 时，每个受影响 target 均须 start、Commit、rebind；旧 blanket live-profile guard 并非完整方案。独立 profile 不合并。

**已 Commit 历史修复与失败区分：**实际 othermachine 问题是在已 Commit 的 `AwaitingReconnect` 后仍保存过时路径 `/opt/homebrew/bin/herdr-desktop-pet`，不证明 Commit 失败，也不授权再次执行 manager。same-image 和合格 successor 修复须逐字节保留旧 plan/journal，仅追加新的 repair/handover receipt；不 replay manager、不手动 Configure、不手动 Resume。保留真实旧证据、helper、reservation、config/state、凭据、provider/account 数据、prompt/transcript 与无关 profile。区分明确 no-spawn 失败、已 Commit 但尚未重连、明确且 quiescent 的 retirement 失败、typed cancellation 与 Unknown。绝不 clear/replay Unknown 或改称 cancelled；真实用户 Stop 禁止包括 rollback 在内的重启。所有必要真实 binding/native receipt 完成前阻止 retirement。文档或旧运行不证明当前 tools Mac 的状态。

## 历史公开与执行记录——不是即将采用的常规路径

以下版本、PID、SHA、取消/Unknown 观察和验收范围保持原有历史含义。当时的“当前”表述、独立 A/B 批准及手动连接步骤不是即将采用的单次批准路径指令。

**Core v0.4.7:** [Desktop Pet v0.4.7](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.7), release `413584934`, source `52514da5115f09aac22de5429c08f7acdd993258` (2026-10-15T08:39:17Z). Six anonymous assets, strict/deep signatures, protocol 4 / observation 2 / API 1 and exact 68-file closure passed. Current observed acceptance: [v0.4.7 ledger](releases/v0.4.7.md). Pages is not yet observed; Wiki publication lacks dedicated `WIKI_PUBLISH_TOKEN`.

**历史 v0.4.7 路径（已取代；当时公开 Brew A′/B 仍待验证）：**运行中 v0.3.10 → 独立 native 同意的常规 v0.3.11 更新 A′ → 保留原始匹配签名 helper 确认 terminal/quiescent settlement → 单独 source review 与最终可见 native **Migrate** 同意进入 Core v0.4.7 B → 精确 profile ready/Commit → 实际所属 manager retirement。当时 A′和 B 需要独立同意。v0.3.6 兼容路径独立；beta 须先采用冻结匹配 v0.3.6 pair。

**同意与恢复：**v0.4.7 区分 bound typed Cancelled 与 VisibleApproved。意外 native 响应、EOF、invalid/foreign/nonzero 或不一致 delivery 保持 Unknown；不得 clear/replay 或将 Unknown manager 改称 cancelled。用户 Stop 禁止包括 rollback start 在内的重启。保留原始 helper/journal/reservation、精确 config/state 及无关 profile；仅在 target ready/Commit 后 retire 已审阅的所属 source。v0.4.6 final-cancel false-Unknown 证据保持不可变历史记录。当前手动 candidate→公开 v0.4.7 观察见 ledger；公开 Brew A′/B、ManagerUnknown、late Stop 与 definite retirement failure/Resume 仍待验证。

**Historical execution records / 과거 실행 기록 / 過去の実行記録 / 历史执行记录:** The older operations below retain their original versions, PIDs, hashes and Unknown scope; they are not v0.4.7 / v0.3.11 runtime acceptance.

**实际公开 Brew v0.4.4 → v0.4.5 常规更新完成：**operation `a6991e69233377c32b150186c62888fd`，`homebrew_upgrade` / `manager_exited`，installed/applied true。PID `10377` / instance `37953e017fc9208bfcc785a4ff138494`、Brew 派生 image `a06f7e7045a1cfa83a3d7ed7d7a135eab4f8d911a3275647b89743fe5815c500` 在精确旧 private profile、已保存 Herdr(H1/T0)上达到 UI/control ready。更新后 native Check 为绿色 **No update from selected source / 0.4.5 / Homebrew**。应用前 Check 仅改变 source preferences；应用保留全部旧 config/evidence 与无关默认 tree，例外仅 `source.log` 和新 operation 文件。无关历史 hidden PID `2641` 只在封存旧 A/B 证明后显式 Stop。没有重装 Herdr。 Actual manager `/opt/homebrew/bin/brew`: PID = PGID `9853`, exit 0, drained true; retained helper strict-signature result 0, stdout upgraded only desktop-pet v0.4.4 → v0.4.5.

**手动所有者修复（非自行更新）：**被阻止的公开 v0.4.3 checker 没有自行更新。签名原应用备份、显式 Stop `9340`、固定公开 v0.4.5 prebuilt 安装、无自动启动检查和显式 Start 后，实际 PID `13055` / instance `93f6048e2ae1266c9d5a42ede0f2da51` 在原 profile、已保存 Herdr H0/T0、精确公开 native image `693bad6245d28be4350b661f018e34416ca950b37f49a6a4a3967c5d1a67eacb` 上 ready。Start transport 在20秒 timeout 后未重试，随后观察到真实 GUI/readiness。除 daemon log 外全部旧 config 与321个 history 文件保留。最终 native GUI Check 已为绿色 **No update from selected source / 0.4.5 / Standalone installation**，原 v0.4.2 history 保留。最终 Check 仅改变预期 preferences，没有 Start 重试或 Unknown journal replay。

**实际全新公开 managed v0.3.6 → v0.3.10 A → v0.4.5 B：COMPLETE。** Genuine private Herdr 0.9.3 / protocol 22 安装精确冻结 v0.3.6 pair。Native Check 仅改变 source preference check metadata，未改变安装/process 或其余四棵 tree 字节。独立 A 同意完成 operation `20cca91548584437af112bb04cc260b0`；保留的签名 v0.3.6 helper 在 B 后仍报告 completed / manager_exited / installed=true / applied=true。精确公开 bridge v0.3.10 PID `2894` / instance `8f8f97c36aed6625166a679396596a66`、raw image `0b1c8e63c6cee6904208bc3d90f184806c8f1216b8114a2b30da009d92be65a6`、protocol 3 / observation 1 / API 1，达到真实 native UI/control ready、Herdr 1 / Tern 0。Bridge Check 结清 A，实际绿色 **No update from selected source / 0.3.10 / Herdr plugin manager**。

独立 Move preview、strict/deep 验证公开 v0.4.5 protocol-4 native **Migrate** 同意仅批准一次 B `6ee850aa5577314c6d9cf6ebc7eae981`（plan SHA-256 `b6fe929da173e648fd2ad8e2845b7fda00461e3e6002e7bb9113155d403127de`）。Durable phase=complete、native_ready=true、target_committed=true、error=null。Source `2894` 确认 Stop，Core v0.4.5 PID `3594` / instance `2ee0995b8b3dc3abe18e6238a91929d9` 在同一原 private config/state、已保存 Herdr、Herdr 1 / Tern 0 上真实 UI/control ready。复用合格 Brew image `a06f7e7045a1cfa83a3d7ed7d7a135eab4f8d911a3275647b89743fe5815c500`，`target_manager:null`，没有中立 install/upgrade manager。Genuine Herdr retirement PID = PGID `3648`、exit 0、quiescent=true 安装不可变 integration v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`；enabled registry、clean checkout 均匹配精确 ref，有全部21 actions/startup/client.attached，旧生成 app 不存在，没有新 integration tag。四棵封存 tree 的全部已有常规文件（source config 5/state 164；无关默认 config 5/state 2）、**全部87个原 A 文件**、helper/plan/records 在 B 后 byte-identical，**没有旧 daemon-log 例外**；新 B 文件仅追加。无关默认 PID `1971` / instance `27752246b4da5c6f3964a74dd22881a2` 保持 running+Hidden、Auto/H0/T0，没有 Stop/restart/merge；adoption 仅在 private XDG namespace 内。B 后 native settings 返回 exit 0 和同一 ready instance。实际迁移 PID `3594` 的 native B 后 Check 为绿色 **No update from selected source / 0.4.5 / Homebrew**，同时确认 migration/legacy retirement `6ee850aa5577314c6d9cf6ebc7eae981` 完成。仅 source `preferences.json` check metadata 改变；包含87个原 A 文件和全部 B proof 的当前239个 state 文件及无关默认 tree 均保持 byte-identical。Pages 部署与四语言公开 render 需要独立观察验证；native 验收/source 编辑不是部署证明。 下方 v0.3.9/v0.4.4 operation 仍为历史证明。

实际固定 beta route **经公开 v0.3.9 entrypoint PASSED**：原 protocol-2 archive `d49824778dc0d898b4386cc04a4fbed34da40ebfe990148441d365984555d1aa`、live/direct-bridge guard、冻结匹配 v0.3.6 pair、无自动启动、显式 ready Start、签名旧 helper 备份；38个 non-app Git 文件、user edits 与 profile 保留。这不是 v0.3.10 beta entrypoint 运行。history 验证分离 private 只读结构检查与精确当前 strict fresh read/write/execute/startup/resume authority。修复前失败的2项回归和全部70项 coordinator test 通过，实际 v0.4.5 只读旧 history probe 保持 config 与321个 state 文件哈希不变。旧 Unknown 仍受原 scope/evidence 约束且只读，绝不 clear、replay 或 promote。

**实际全新公开 Brew v0.4.5 安装通过：**安装前记录了全部中立 app/config/state/process root 的真实缺席（74个文件 / 44.3 MB、五项内部/外部重新签名、没有升级其他 formula）。合格只读 helper discover 验证精确 Brew 派生 image `a06f7e7045a1cfa83a3d7ed7d7a135eab4f8d911a3275647b89743fe5815c500`；安装/检查没有创建中立 config/state、standalone app 或 process。**只有显式 Start 后**才出现默认 PID `1971` / instance `27752246b4da5c6f3964a74dd22881a2`、v0.4.5 / protocol 4 / capability 2 / API 1、Auto / H0/T0 和实际 UI/control ready。实际 Settings 显示 Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0 与内置 Rubelia。显式 Hide 保持同一默认 instance hidden 且 running，与已完成的新 A/B 无关。全新安装不是 managed A/B 验收。

**历史 v0.4.4 全新 Brew 安装证明:** 中立 v0.4.4 配方已公开于 tap `2aa9a4be9517e0232a3f2b8e2b3c748045c69586`。合格 Brew v0.4.4 全新安装通过（74个文件 / 44.3 MB，五项内部/外部重新签名），认证检查确认 Homebrew 派生 image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312`。安装/检查没有创建中立 config/state 或 standalone app，也没有启动守护进程。仅显式手动 **Start** 后才达到实际 v0.4.4 UI/control ready，PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470`。露贝莉娅及实际 Settings 显示 **Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0**；随后显式 Hidden 保留同一默认实例。这是全新安装证明，不是 v0.4.4 常规更新或 managed 迁移完成证明。

**Historical public bridge v0.3.11 publication:** [Genuine Herdr bridge v0.3.11](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.11), release `413623057`, tag object `a907424e89c10d484f8646f41d5f020fa93ba8bc`, built from exact public Core `52514da5115f09aac22de5429c08f7acdd993258`. Five anonymous assets, genuine legacy bundle/executable/helper, protocol 3 / observation 1 / API 1 and exact 68-file closure passed. At that publication, stable tap `6693cd9db26e6dba9a257bbe76acaf78cb502c2f` served Core v0.4.7 / bridge v0.3.11; beta was unchanged.

**历史 v0.3.9/v0.4.4 managed A 完成:**冻结 v0.3.6 Check 选择公开 v0.3.9，仅改变 `preferences/last_update_check`（`1791963743` → `1791965310`）；Check 期间其余四棵 tree 字节、source PID `2467` 及无关默认 PID `2641` 不变。精确 source `0f33f28` native A 同意完成 operation `d5b2bf6f02e031eabb5a8c8fdd805cab`（`completed` / `manager_exited`，installed/applied true）。实际公开 v0.3.9 native PID `3417` / instance `b42b25d48315e66bd87e7685e58cfb14`，image `0ed12d159b6bbb83ada833d39b4524ac5f7eccb1253b7acaf9aa2b71cbf833ce` 在精确旧 private config/state 上达到配对 protocol 3 / capability 1 / API 1、UI/control ready、Herdr 1 / Tern 0。实际 managed source Git status 完全 clean。保留的原始 v0.3.6 helper 通过 strict/deep 签名，实际 status 确认 completed。baseline 已保存全部87项旧 A 证据和91个 state 文件哈希。

**历史 v0.3.9/v0.4.4 managed B 完成:**单独的 Move preview 与独立 strict/deep 验证通过的公开 v0.4.4 coordinator 收到 native **Migrate** 同意，operation `d729fd740f4e459945edda00bf48d944`、plan SHA-256 `6471e928254229a66a6fc64a88f67095777d6bd890d621feb36b08a4c7d1fc69` 达到 **COMPLETE**。Native Prepare/Stop 停止公开 v0.3.9 PID `3417` / instance `b42b25d48315e66bd87e7685e58cfb14`，选择合格的既有 Brew v0.4.4 app root/image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312`；`target_manager:null` 表示**没有重装**。Target PID `4104` / instance `8912b3aad5419b1c14a78ef3418ae36f` 在精确旧 private config/state 上保留已保存的 Herdr 选择（Herdr 1 / Tern 0），达到 protocol 4 / capability 2 / API 1 及实际 UI/control ready；native_ready 和 target_committed 均为 true。真实 Herdr retirement attempt 1 为 PID = PGID `4140`、exit 0、quiescent true、error null。实际 plugin list 显示不可变 Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8` 的 enabled integration-only Desktop Pet，managed Git HEAD 一致；真实 manager 输出为 v0.4.1、21 actions、startup/client-attached hooks。Managed dist/app 及旧 PID `3417` 消失，未创建 standalone `DesktopPet.app`。精确 retired v0.4.1 标签的历史 Core v0.4.2-minimum wrapper 实际复用同一 v0.4.4 PID/instance，没有新 app 或 tag。全部87项原始 A 证据 SHA-256 及所有既有旧 source-config 哈希不变；无关默认 config/state 两棵 tree 字节相同，隐藏默认 v0.4.4 PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470` 不变且仍运行。仅 adopt private XDG base profile，未与 global default 合并。实际 GUI 已目视确认 Desktop Pet / 内置露贝莉娅及 Settings **Herdr Connected 1 / Tern Disabled 0**；clean-image capture 仍待完成。无 fault 的最终公开 v0.3.9/v0.4.4 A/B happy path，与已单独通过的历史 working metadata `f7edf5e` definite failure/显式 Resume 验证区分。

**历史 v0.4.7 consumer / SDK 记录：**Herdr integration 当时为 assetless v0.4.1 (`6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`)。本地 consumer scripts/8项 evidence 来自公开 Core v0.4.7，当时尚未 push main，无新 integration tag 或 consumer native build。手动 same-source v0.3.11 candidate → 公开 Core v0.4.7 B 通过，但不代表公开 Brew owning-manager retirement。当时 consumer 需要对同一 executable/config/state 执行 Configure → Save → Connect；account/provider 与两种启动顺序验收未完成。[历史观察](releases/v0.4.7.md)不是当前 Mac 状态证据。

**Historical v0.4.5 consumer evidence:** **consumer 与 SDK 边界：**Herdr 集成保持公开 v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`（无资产 release `412252863`）；历史 Core v0.4.2 minimum 冻结不变，健康 Core v0.4.3 复用已有实际观察。当前 Herdr consumer scripts 及全部8项 metadata/evidence 文件现已从公开 Core v0.4.5 release `412667385` / 源码 `51dcf22415b1ff0faf07651cb83c1c696c980f43` 实际 materialize；没有新的 Herdr 标签；最终公开 managed retirement 及不可变 wrapper 对 v0.4.4 的复用已由上文实际 B 结果单独确认。Tern v0.2.0 及兼容 Core v0.4.1 pin 不变。Tern 必须显式 **Configure → Save → Connect** 验证可执行文件及实际 config/state 对；不会安装或自动启动应用。真实 Tern keychain/provider 登录和 genuine OMP 18.10.3 providerSignIn/NoModel 仍阻止两种启动顺序的同实例/prompt/transcript/reconnect 验收。不得复制凭据或伪造账户、ACK、完成结果来替代这些关卡。**完整 matrix 尚未完成。** 实际当前 Herdr v0.4.5 reviewed-consumer VM smoke 通过：materialized wrapper 发现同一 Brew v0.4.5 PID `10377` / instance `37953e017fc9208bfcc785a4ff138494`。Hide 后 ensure 不 bootstrap，保持 Hidden / visible false 与同一 instance；显式 Show 恢复显示。当前 Herdr source 验证通过：integration-only ownership v0.4.1、shell/manifests/TypeScript、固定 contracts `85c644cd90d8c6a9e97e9dc313cc4750f344235f`、**104项 Rust adapter test 与一项已有 test 的 child 重跑**（不是105项不同 test，也不是 Bun）；未构建 native app/release。

**历史 v0.4.2 / v0.3.7 证据（不是 v0.4.4 运行证明）：** **发布边界：**[Desktop Pet v0.4.2](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.2) 已公开（release `412241744`，`2026-10-14T01:09:50Z`，不可变源码 `cb10ce2c3cc65402cbb914d60301f7627d5546d5`）。已验证六项匿名资产、strict/deep 签名、protocol 4 / capability 2 / integration API 1，以及全部 68 个部署资源哈希和精确 closure。tap `32919c745864b54a2e716fcc5a3714d0d38d3e84` 的中立配方及实际公开 v0.4.1 → v0.4.2 Brew 升级证明了 receipt/derivation、五项签名、`brew test` 和 Auto 原生 UI/control ready（Herdr 0 / Tern 0）。全新公开 standalone v0.4.1 安装与两个并发安装器复用运行中所有者已验证；原生 Check 提供实际公开 v0.4.2，经同意完成 `standalone_replace`、`manager_exited`、`installed:true`、`applied:true`，相同 config/state 和精确公开 v0.4.2 image 准备就绪。Check 保留 locator/profile/lifecycle/locks/PID，但改变了 `preferences.json` 检查缓存，不能声称所有配置字节不变。[修正 integration-only Herdr v0.4.1](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.4.1) 已公开（release `412252863`，不可变 ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`）。两个公开 wrapper 保持隐藏并复用相同 Brew v0.4.2 PID/instance/image；显式 config/state 加真实 private patched Herdr 0.9.3 socket 达到 Auto Herdr 1 / Tern 0。全新环境的两个公开 bootstrap 安装/复用一个 OS-home 应用，无集成内 native 文件，也未启动守护进程。这不是公开 Herdr manager 安装 receipt 证明。原始 Herdr v0.4.0 的生成应用/失败测试 scaffold symlink 源码缺陷保留，不能称其为 integration-only。[Bridge v0.3.7](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.7) 已公开（release `412267469`，不可变 tag `987a4b859f95e922c1c5443ead372d10d0dddb24`）。来自精确 Core `cb10ce2` 与修正 Herdr `6d2ac04`，五项匿名资产、签名及 68 文件 closure 已验证。tap `657f32cc4cc563ae40ae0432ac5123dcb3f58426` 只将 old stable 配方更新为 v0.3.7，beta 不变。manual-origin private 真实宿主运行达到 Herdr 1 / Tern 0 ready，但不是 managed A→B 证明。实际冻结公开 Brew v0.3.6 → v0.3.7 A `6a10663c2d2f0177990623a969238af4` 与单独同意的 Core v0.4.2 B `c66961f9e5a91c0023545065ae61385b` 已完成，包括真实 Brew retirement（exit 0、quiescent）。B 前后所有配置文件哈希与旧 A 证据的 87 个文件均不变；实际 Core v0.4.2 UI 显示 Desktop Pet / Herdr Waiting 0 / Tern Disabled 0。相反，真实 managed v0.3.7 A `a1aec5902020de7bcaaf583b1a5f50a5` 在 native `/tmp` 配置目录验证处失败：checkout 替换/registry 注册前 manager exit 1，冻结 helper 保留 Unknown / `manager_exited`、installed/applied false。VM、journal、reservation 保留，未 recovery/replay。

**历史公开 v0.3.8 managed 结果与安全 successor 边界：**managed v0.3.6 → 公开 v0.3.8 A 已完成（`16b6307413ec409efe3eb5a04c9cc21f`），原始签名 v0.3.6 helper 确认 completed status。显式 Check 后 B preflight 以 `managed source changed; migration never resets user changes` 拒绝：`dist/HerdrDesktopPet.app` 的生成文件恰好68个为 untracked。这发生在 B 同意/Stop 前，没有 B manager/fault flag，当时 source PID `3563` 不变。原始公开 v0.3.8 标签恰有10个常规 UTF-8 Git 文件且没有 `.gitignore`；此 B 限制是历史事实，公开 v0.3.9 不会追溯修复旧标签。不要手动 ignore、`git clean` 或 reset 来让旧标签通过。已完成 managed v0.3.8 A 的用户，必须先用保留的**匹配签名 helper**确认已知 terminal 旧 operation，备份实际 config/state 与旧证据，显式 **Stop** 源实例，再由**真实拥有该安装的 Herdr manager**显式安装 successor。绝不 replay Unknown manager operation；保留 VM/journal/reservation 供协调。

**历史 working-metadata 与常规更新证明（不是最终 v0.3.9/v0.4.4 验收）：**真实 managed B 使用 working metadata `f7edf5e0eba374248b9a5ee9d66d26109b39eca2`、不变的公开 v0.3.8 native 和公开 Core v0.4.3，在 NativeReady/Commit 后达到 definite retirement failure（manager exit 1、quiescent）。仅移除引入的自有 `uchg` flag，再以同一 plan SHA `3615fa140fd1b0d49910f5f2ef170b64122243c6c65d0098d4ecfd729e951f16` 获得独立显式 Resume 同意，attempt 2 完成（真实 CLI exit 0、quiescent）。旧 managed app/source 已 retirement，Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8` 复用同一 v0.4.3 instance。全部87项旧 A 证据、5项 first-attempt/plan/consent 文件、旧 config 及无关默认 profile 哈希不变。实际公开中立 v0.4.2 → v0.4.3 常规更新亦已完成（`81c82d837b65fb03584bfb1e668119c3`，`standalone_replace`），同 profile/Herdr 0 ready 及露贝莉娅 UI，无 Herdr 重装。Check 仅改变 `last_update_check`；Check 后全部 config bytes 及87项旧 A 证据在更新前后不变。这不证明最终公开 v0.3.9 A/B 或 v0.4.4 更新。


## 系统要求

Apple Silicon Mac、macOS 13 或更新版本。源码构建需 Rust/Cargo、Bun、Node.js/npm、Xcode Command Line Tools，预构建无需这些工具。应用为 ad-hoc 签名，未经公证。Herdr 快照需兼容本地服务器（0.9.0+），回复另需 `agent.prompt`。可选 client-attach 自动启动需官方 Herdr 0.9.3 应用[集成仓库的补丁和宿主构建指引](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)，或实际支持钩子的宿主。原版 0.9.0/0.9.3 没有该钩子，插件安装不修改宿主。

## 安装与可选集成

**历史 v0.4.4 全新 Brew 安装证明:** 中立 v0.4.4 配方已公开于 tap `2aa9a4be9517e0232a3f2b8e2b3c748045c69586`。合格 Brew v0.4.4 全新安装通过（74个文件 / 44.3 MB，五项内部/外部重新签名），认证检查确认 Homebrew 派生 image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312`。安装/检查没有创建中立 config/state 或 standalone app，也没有启动守护进程。仅显式手动 **Start** 后才达到实际 v0.4.4 UI/control ready，PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470`。露贝莉娅及实际 Settings 显示 **Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0**；随后显式 Hidden 保留同一默认实例。这是全新安装证明，不是 v0.4.4 常规更新或 managed 迁移完成证明。安装命令：

```sh
brew install hanbong5938/tap/desktop-pet
desktop-pet --version
desktop-pet start
desktop-pet status
```

预构建与源码安装在 **core** 检出目录中执行，而不是 Herdr 集成目录。默认仅预构建，下载或验证失败不会暗中回退源码编译。`--source` 明确仅构建当前 core，并使用该目录的 `dist/DesktopPet.app`。

**历史固定 v0.4.7 安装示例（不是最新版指令）：**下列命令有意选择旧不可变版本。当前公开依据为 v0.4.8，不得替换为未公开的 v0.4.9 资产。

```sh
git clone https://github.com/hanbong5938/desktop-pet.git
cd desktop-pet
# Published immutable v0.4.7 installer and pinned assets:
git checkout --detach v0.4.7
bash scripts/install.sh --prebuilt
# 若选择从同一个公开 v0.4.7 标签安装源码：
# bash scripts/install.sh --source
# Replace with the installer-reported executable and your actual profile paths:
/absolute/reported/desktop-pet --config-dir /absolute/dev/config --state-dir /absolute/dev/state start
```

不可变 v0.4.7 的默认／`--prebuilt` 使用精确公开固定资产（历史 v0.4.0／v0.4.1 保留各自 pin）。下载／验证失败绝不暗中构建 source。显式 `--source` 构建所选 core 检出，包括实际公开 v0.4.7 标签。 standalone prebuilt 使用**实际 OS 账户 home** 的 `Applications/DesktopPet.app`，获准 source 使用 core 的 `dist/DesktopPet.app`。admission 可复用已验证健康现有所有者；请使用安装器报告的执行文件，勿假定 dist 路径。共享 locator 原子保存到账户 home `.local/share/desktop-pet/installation.json`，只是发现证据而非权限。无效显式路径／locator 不回退，执行文件／签名／capability／origin 每次重新验证。Brew 拥有实际 formula／prefix，不暗中接管 live／已预约的安装。

仅安装需要的集成。Herdr 是轻量消费者；Tern 不下载、构建、安装或自动启动原生应用。插件更新与卸载只影响集成，不删除共享应用、配置档或角色。常规应用更新不重新安装 Herdr 插件。

```sh
herdr plugin install hanbong5938/herdr-desktop-pet
tern plugin install https://github.com/hanbong5938/tern-desktop-pet
```

## 配置档、连接与迁移

新配置档默认为 Auto。Connection 显示已注册提供商的实际连接，而非安装检测。提供商 ID 是不透明值，操作权限来自 capability，而非名称前缀或路径。Auto 是同时观察而非回退选择。远程来源只读；Tern 允许回复，不允许删除工作树。接受提交不代表代理完成。

参阅[profile 与发现契约](development.md#profiles-and-installation-discovery)及[迁移与保留历史](migrations/desktop-pet.md)。即将采用的迁移自动交接实际 executable/config/state，并在同一 Tern 窗口 fresh rebind 后才 retire。不得额外加入手动 Configure/Connect。迁移之外的首次独立连接仍可显式配置。现有 Tern、beta、自定义 profile 原地保留，绝不合并。

```sh
desktop-pet profile show --json
desktop-pet --herdr-socket /actual/path/to/herdr-api.sock start
desktop-pet preferences set --observation-host auto
```

## 操作

| 操作 | 效果 |
| --- | --- |
| 左键点击菜单栏图标、键盘激活或 AXPress | 打开已有的三标签设置面板（角色、观察来源、气泡/设置）；再次左键将同一面板置前，不关闭也不重置标签。使用兼容 macOS 13 的现行系统 API 请求激活，但系统不保证键盘焦点转移。 |
| 右键/Control-点击菜单栏图标 | 先隐藏面板再打开原生**设置…**、**退出**及必要时的**显示并启用角色**菜单。 |
| 点击头部/身体、在头部往复移动 | 触发反应、抚摸角色。 |
| 拖动角色身体/背景、按 Option 拖动角色窗口；右下角把手 | 移动或调整角色大小。 |
| 拖动气泡背景 | 附着时移动角色；独立时仅移动气泡。 |
| 拖动气泡边缘/角落（可按 Option） | 边缘调单轴，角落调双轴，与角色缩放/字体大小无关。 |
| 右键/Control-点击气泡背景或卡片标题 | **关闭气泡窗口**仅隐藏气泡；**恢复自动大小**清除紧凑和展开大小覆盖。原生复制/粘贴及其他控件菜单不变。 |
| 全窗口/透明区域点击穿透 | 全窗口模式同时穿透角色和气泡而无法交互；透明区域模式仅穿透透明角色图像，气泡仍可交互。 |

**本源码检出：**左键捕获中断后于松键时静默恢复，不伪造点击反应。新按键有自己的拖动锚点；重复或过期事件不能替换锚点或结束别的窗口手势；Control-左键释放不能结束先前点击。普通点击、Option 拖动和缩放把手保持不变。丢失释放回调的有效气泡缩放，在下述跟踪/IME 完成保护下仅接纳一次预览。要获得此变化需重新构建安装本检出；已安装的发行二进制不变。

紧凑/展开气泡分别保存大小请求，附着和独立模式共用。预览和完成时，对边/对角在拖动开始的显示器上固定；之后附着布局跟随角色。拖边只改变一个请求轴，重新换行却可抬高另一显示轴的最小值。高度按真实屏幕/锚点所限宽度测量，不降低用于持久化的固有最小宽度。仅沿非活动轴移动或活动位移回零不会改写请求；完成时依据最新自动/手动下限重排，可增可减。展开消息与卡片共享高度预算，先预留卡片/回复最低高度及一行消息，再分配余量。自动布局保留总体高度上限，手动请求不受该自动上限约束；内容放得下时隐藏滚动条，溢出时仍可滚动。小屏只限制显示尺寸，不丢弃请求。

附着气泡的整个主体避开角色稳定边界，不仅避开头部锚点。**自动**只要当前侧还能容下回复/控件最低尺寸就保持该侧；任务展开、打开回复和实时重排不重启选边。可按该侧余量限制显示大小而不覆盖保存请求。附着缩放预览/完成使用捕获侧的安全区域，并固定对边。若各侧均放不下最低尺寸，则在角色窗口前方使用受限、无尾巴、最小重叠的回退；不移动角色也不修改保存大小。

打开回复会先测量附着布局再聚焦，使所选行、输入框及光标可见。重排保留原生编辑器、草稿和选择。IME 组合期间等待内容/布局变化，不重设编辑器父级或大小；如屏幕/角色几何变化使当前窗口不安全，组合结束前仅允许安全移动窗口位置。

手势之外仅当气泡真正位于指针下，重排才更新缩放光标；原生文本/控件自有光标，不会重置其他窗口/角色上方光标。大小位移为零但独立气泡实际原点改变时仍保存原点；请求和几何都未改变则不写首选项。回复 IME 正在组合时不能开始缩放或**恢复自动大小**；按下后开始组合会暂停预览，松开/取消会停止跟踪，但等组合安全及跟踪结束后才保存并应用待定更新，卡片和草稿保持不动。完成依据最新内容、语言、布局和屏幕裁剪只保存一次。保存失败会恢复原有大小请求、实际独立原点和待定状态，不产生幽灵保存，虽然新内容可改变显示下限。恢复自动大小保留位置、可见性、模式和草稿；重置位置/颜色不删除大小请求。回复有标记文本时菜单栏图标不能直接打开设置，请先结束组合；Escape/点击外部仍可关闭设置面板。

## 会话搜索和排序

卡片优先采用观察到的终端标题，再用命名标签，最后用工作区/目录上下文；无名称有明确回退，重复名称显示区分标记。悬停查看完整标题、工作目录和内部 ID。标题变化保留选择/滚动；断线卡片保留最后标题并显示**离线**。

**搜索**去除两端空白，对标题、标签/工作区、代理、来源显示名、完整工作目录或备用名做不区分大小写的子串匹配，不搜索 ID 或套接字路径。搜索与**状态**筛选相交，不改变整体观察状态或反应。**排序**可选**稳定**（原顺序）、**名称**、**来源**，分隔线下另有独立的**运行中优先**；所有纳入来源的运行中会话靠前，组内按所选顺序排列。关闭的菜单和辅助功能值都包含两项选择，并非按全部状态排序。GUI 在筛选/排序**后**最多展示 128 张卡片并报告省略数；CLI `sessions list --limit` 是**分页大小**，不是总行上限（[CLI，英文](cli.md#sessions-and-agent-prompt)）。

初始搜索为空，状态为**全部**，排序为**稳定**，运行中优先关闭。只有成功保存的排序/运行中优先在重启后保留；保存失败恢复控件。搜索和状态仅存于内存。筛除选中的回复会折叠输入框但保留该会话草稿，清除搜索不会自动重开。聚焦搜索时 Enter/Command+Enter 从不发送。Escape 先取消搜索 IME 组合，再清除非空查询，最后退出空输入框而不折叠气泡。搜索和回复的组合分别推迟结构性列表变化到取消标记，实时状态仍更新。

## 内联回复与消息传递

内联回复首次出现在 v0.1.6；稳定版 v0.2.0 修复原生回复文字截断和 Command-A/C/X/V 剪贴板分发。展开气泡最初只显示卡片，无回复框，**全部**视图亦然。点击本地代理卡片打开单行输入框；切换卡片会折叠旧框并打开新框。远程卡片只读，无输入框/发送。键入、粘贴或导入已提交的换行会变为空格；IME 预编辑在提交前不动。**Enter**、**Command+Enter** 或**发送**可提交，但组合期间不能提交。**Escape**先取消组合，否则只折叠输入框；点击气泡外也折叠（组合期间除外），点击气泡内其他位置不折叠；两者均不关闭气泡。会话草稿跨输入框/气泡折叠保留在内存，不跨重启。成功仅清空/折叠当前仍匹配的回复；发送期间编辑或改选会保留新草稿/输入框。失败或结果不确定也保留草稿和回复状态。

IME 组合时选择、筛选变化和结构性行替换等待，不丢失标记文本；取消标记后应用最新筛选及仍可见有效的选择。已提交文本始终属于原会话和连接代次，实时状态/发送资格仍更新。输入框有清晰边框和来自配色的非透明背景；紧凑原生发送按钮随资格变化，改主题保留草稿/预编辑。

Herdr 卡片传递**仅使用**所选本地 Herdr 来源的 `agent.prompt` API，不调用全局 CLI、shell、SSH 或原始 pane 输入。排除本地观察、离线来源、过期/未就绪会话或不支持的服务器都会禁用发送；`unsupported_method` 会报告错误，不采取绕过措施。保留的远程卡片亦不能发送。审批/提问请在真实终端处理。`agent.prompt` 已在 Herdr 0.9.2 验证；本地观察最低 0.9.0 并不证明运行的服务器支持它。提交 ACK **不是**代理完成；不自动重试，不确定时先核实再手动重发。API 发送给当前 pane 占用者，没有原子预期会话保护，瞬间更换占用者/会话仍可能发生。自动化参阅[CLI 会话身份和发送（英文）](cli.md#sessions-and-agent-prompt)，它不是 GUI 回退。

Tern 回复使用注册适配器重新验证后的**单次 `agents:ask`**，绝不回退 Herdr、shell 或 PTY 输入。`prompt_accepted` / `accepted_may_be_queued` 只证明 SDK 接受，不证明模型收到或完成；已提议／不确定的 ask 不重放。本地窗口与分组宿主清单保留适配器授权的 locality；远程卡片只读，Tern 不授予工作树删除权限。窗口来源五秒后过期并显示 Offline，Auto／无适配器的应用继续运行。清单分为四个协作式 zero-delay 回调，ask 结果后立即交换新快照；单个 SDK 调用或 OS 抢占仍可超出预算，不保证延迟／稳定性。

## 可见性与菜单栏恢复

角色的 `show`/`hide`/`toggle` 与**气泡 → 显示气泡**的 `show_bubble`/`hide_bubble` 独立。两者可见时气泡附着；仅角色可见时隐藏气泡；仅气泡可见时成为可移动无尾巴的独立卡片/回复气泡；两者隐藏时应用和 Herdr 会话仍运行。**关闭气泡窗口**只修改保存的气泡可见性，不删会话/内存草稿；取消上下文菜单保留卡片选择/草稿。回复 IME 活跃时禁用并复查**设置…**及**关闭气泡窗口**，先完成组合再开菜单。通过角色/独立气泡的上下文设置或可见菜单栏图标重新进入**气泡 → 显示气泡**；独立气泡菜单还提供**显示角色**。

隐藏角色不改保存的气泡可见性。首次脱离且无独立原点时，采用**实际应用过的附着气泡主体**位置，而非仅计算出的位置；若角色一开始隐藏，普通附着布局提供回退。独立原点与角色位置分开保存，之后即使有标记文本也使用该原点。拖背景仅移动独立气泡；重新显示角色会附着，再隐藏又恢复原点。紧凑/展开、回复和重新测量都保留原点（屏幕裁剪除外）；显示器移除/布局改变会救回屏幕外位置。选择、视图/回复状态和草稿跨本次隐藏/显示保留，不跨重启。独立时改变的附着位置在下次附着时生效，不移动独立原点。`reset` 清除旧独立原点，需要时从新应用的附着布局重新取值，不复制角色坐标或改变可见性。标记回复文本会将隐藏/显示/位置/reset 的原点提交延至取消标记；较新的 reset 优先于旧原点。编辑器回调观察能证明标记文本、已提交草稿和选择保留，不等于真实 IME/焦点认证。

**设置 → 菜单栏图标**有**始终显示**（稳定版 v0.2.0 起默认）和**仅在需要恢复时显示**，与窗口可见性独立。恢复模式只在**两个窗口均隐藏**或开启**全窗口点击穿透**时显示图标；角色单独可交互、独立气泡单独可交互、两者都可见以及仅透明区域穿透时（除非均隐藏）不显示。始终显示模式则均显示。关机阶段两种模式都不显示；透明区域轮询本身不触发恢复。右键/Control-点击图标仅在需要恢复时提供**显示并启用角色**：显示角色、关闭全窗口穿透，保留气泡可见性与透明区域穿透并重新附着可见气泡。菜单关闭后恢复模式隐藏图标；CLI `show` 只显示角色，不显示气泡。原 `bf23ced` 本地预览的条件图标仅为历史记录，并非当今配置继承的首选项（[预览记录，英文](migrations/unreleased.md)）。

`preferences.json` 的 `menu_bar_mode` 为 `always` 或 `recovery_only`；缺键即默认始终显示，旧条件预览配置也一样。成功修改即时应用、跨重启和其他保存，保留未知字段；保存失败维持原模式/图标。切换模式不改变可见性、点击穿透、位置、草稿或生命周期。

**设置 → 菜单栏图标 → 选择图片…**导入**静态 PNG**（编码 ≤4 MiB、解码 ≤100 万像素）；拒绝动画、损坏、截断及完全透明图片。原色按比例置于 18 pt 内，带 1×/2× Retina 表示。成功托管复制至配置目录 `menu-bar-icons/`，由可选 `preferences.json` 的 `menu_bar_icon.asset` 引用；源文件此后可移动。备份时与 `preferences.json`、`lifecycle.json`、`characters/` 一起备份此目录。未设置图标则显示模板爪印。导入/保存失败保留旧选择；重启时托管图片缺失/损坏则显示默认图标**并报错**，元数据不丢。**恢复默认图标**只改图片，不改模式、窗口、穿透、几何、草稿或生命周期。导入在原生解码前验证**每个 PNG 块的 CRC**（含辅助块和 IEND）、IDAT zlib Adler-32 及颜色/隔行扫描精确解压行长。复制原子发布，不覆盖现有文件。历史中断留下的私有临时硬链接，仅当它经确认是自有、私有且同 inode 的别名，才可通过显式重新导入**相同有效 PNG**修复；启动时不扫描，也不修复任意链接、其他文件或损坏 PNG。历史 beta 证据[单列（英文）](releases/unreleased.md)，不等于真实选择器/VoiceOver 认证。

## 删除工作树

展开气泡后右键/Control-点击**卡片标题**会以该卡片为目标，即使另有选择；点击**背景**会以*打开菜单当时*的所选卡片为目标。无合格选择则不显示**删除工作树…**。菜单标出目标；原生输入/其他控件菜单不变。**关闭气泡窗口**仅隐藏气泡。

确认框列出仓库、**准确检出路径**及**整个工作区**（不只是卡片）的标签/pane 数。删除会关闭其中所有标签/pane、终止代理/进程并删除关联检出，**包括被忽略的文件及构建输出**；不会删主仓库或 Git 分支，也不能移到废纸篓/撤销。**取消**排第一且是默认操作；Return、数字键盘 Enter、Escape 都取消。**删除工作树**是独立第二操作。若原生警告框布局后无法把取消设为默认，操作中止。操作前检查自己拥有且可丢弃的检出，以及所有受影响文件和进程。

仅仍在线、连贯、本地且拥有明确有效关联工作树元数据的来源符合条件；主根目录、远程/保留/离线卡片、缺失/错误元数据及不支持它的旧服务器都不符合。不支持 `worktree.remove` 时报告错误，不调用 Git/shell/SSH 强删。活跃 IME 禁用删除，提交前还会复查。一次只允许一项操作并冻结选中目标；确认后按新快照复查，只发送一次 `worktree.remove` 且 `force: false`，不重试或乐观删行。脏的受跟踪/未跟踪文件或锁可导致服务器拒绝，但被忽略文件**仍可能被删除**。可见行由 5 秒 watcher 刷新；选择/行/窗口变化后反馈仍在。不确定写入结果必须**先独立检查服务器与工作树**，再考虑手动重试。请求指定 `workspace_id`，没有原子的预期检出/代次 CAS；客户端复查不能消除重启/重绑定竞态。[CLI 检查与令牌契约（英文）](cli.md#worktree-removal)增加 60 秒一次性已检查目标，并区分上游 ACK、随后观察 pane/会话消失与实际文件系统证明；切勿拿正在使用的检出试验。历史 beta 观察不是稳定版真实输入认证（[记录，英文](releases/unreleased.md)）。

资格还要求注册来源具有 local／工作树删除 capability，提供商名称或貌似本地的路径不授予权限；Tern 无删除 capability。

## 气泡和角色设置

通过菜单栏面板或上下文**设置…**进入。原生下拉菜单可选择气泡主题与 UI 语言：**系统**按 macOS 首选语言顺序解析；直接覆盖可选韩语（`ko`）、英语（`en`）、简体中文（`zh-Hans`）、日语（`ja`）。系统语言识别日语和简体中文（`zh`、`zh-Hans`、`zh-CN`、`zh-SG`）；显式繁体（`zh-Hant`、`zh-TW`、`zh-HK`、`zh-MO`）不会映射为简体，而是继续查找下一种支持语言，最终回退英语。保存的规范语言值只有 `system`、`ko`、`en`、`zh-Hans`、`ja`。主题/语言即时保存，自定义颜色需点**应用颜色**。窗口打开时安静的颜色冲突检查只更新控件资格/标签，不改草稿、色块、焦点、选择和撤销；关窗停止。原生下拉菜单、滚动条或拖动跟踪期间推迟气泡显示/位置变化，结束后应用最新设置/状态。

**气泡 → 状态图标和颜色**默认开，立即生效并持久化；关闭恢复纯文字。运行中蓝色、等待琥珀色、**已验证**成功绿色勾、失败红色。`Done`/空闲但未接受结果则中立，不推断提供方成功。匹配的已验证终结报告可跨空闲/快照维持；新活动、会话身份模糊、报告丢失或断线会清除，身份恢复也不会再次接受同一缓存报告。新来源（包括空快照）在首个连贯快照前一直离线。摘要绿色表示**每个已观察会话**成功验证，与卡片筛选/展示上限无关。状态始终保留文字；图标在辅助功能中为装饰，卡片不重复子节点而呈现完整标题/状态。切换图标不改卡片选择、筛选或草稿。透明区域穿透靠指针轮询；若不能接受点击路由竞态，请关闭。

**角色**标签有精简的当前角色/候选/操作摘要；无候选且无有意义反馈时隐藏空选择/状态卡片。浏览器和操作详见下文。角色标签高度随内容、标签/状态变化，而气泡/设置保留既有高度/滚动；小屏限制并滚动真实内容。角色包 v5 和 `default@0` Rubelia 未改（[v0.3.3 修复，英文](releases/v0.3.3.md)）。

## 编辑角色对话

选择**角色 → 编辑对话…**，打开可调整大小的独立深色编辑器，默认内容 760×560、窗口最小 620×480。不激活也可选择任一可用角色及**原作者修订版**；语言独立于 UI，可选韩语（`ko`）、英语（`en`）、简体中文（`zh-Hans`）、日语（`ja`）；八种事件分别为空闲、运行、等待、未知、点头、点身体、抚摸、观察到任务完成。控件在顶部、事件在左侧、编辑和可展开的原文参照在中央、状态/操作固定在底部；悬停看完整截断的角色名/错误。未保存条目有标记。无覆盖时初始编辑框显示原作者文本，并提供单独参照。用户文本只替换这一条对话，不改观察数/状态转换、反应时机或断线提示。

**保存对话**/**Command+S**每次保存一种语言的一个事件，最多 **2048 UTF-8 字节**（不是字符数）。空/纯空白删除该覆盖；回退到角色包/默认/状态文本，不隐藏气泡。活动角色 ID 的覆盖即使编辑历史修订版也更新适用气泡；编辑别的角色不会激活它。**重置此条**在丢弃未保存草稿时要求确认；**重置全部角色对话…**总是确认并删除**所有四种语言**的覆盖。草稿按角色/语言/事件隔离，跨上下文或 UI 语言变化及本次运行的关窗重开保留；仅保存项跨重启。**Command+Z**/**Shift+Command+Z**是按编辑上下文的撤销/重做；保存失败保留草稿和旧已提交文本。

覆盖存于 `preferences.json`，**不存角色包文件**。内置/导入角色按 ID 跨修订、更新、恢复共享。外部 `--assets` 使用单独的规范路径命名空间；本次运行中激活时捕获的身份即使原目录改名仍可编辑，但重启后改变路径**不会迁移**覆盖。如果两个托管登记册均损坏且存储转为只读，仍可在当前列表代次读取内置原文，而不激活/修复登记册；不可用的托管修订版不会凭空恢复。

外部保存不能悄悄覆盖编辑器草稿：修订版/条目过期冲突会阻止保存。**重新加载已保存**明确采用最新已提交文本；**重定基准草稿**保留本地文本，对新基准重新验证后才能保存。冲突保留文本、焦点/选择和撤销；IME 标记文本将重载/重定基准推迟到取消标记。准确的 `(target, reference)` 选择或不可用参照和草稿维持原状，不悄悄换修订版。首次就绪元数据即使输入框聚焦也填充未触碰字段；就绪前原始编辑（含空白、IME）保留，延迟填充在取消标记后解决而不覆盖随后的焦点/选择/撤销。自动轮询维持缓存的元数据错误，显式重开/选择或新读取可再试一次。CLI 的整个身份 JSON、原作者参照/代次及可选基准保护同一安全边界（[对话 CLI 契约，英文](cli.md#dialogue)）。这些冲突控件晚于 v0.2.0/v0.2.1，旧二进制未补入。

## 角色包

Rubelia（`default@0`）是**唯一内置角色**；可选 PNG/rig 与服装包在独立的[角色图库](https://hanbong5938.github.io/herdr-characters/)中。**角色 → 浏览角色…**打开独立、保留状态、可缩放的原生浏览器。按名称/ID 和官方标签搜索，筛选**全部**、**已安装**、**官方目录**。静态肖像只是预览，不是包归档；浏览不会下载归档。网络/目录错误不影响内置和已安装包的管理，却禁用官方下载；格式不兼容不能下载安装。合格的已发布官方卡片上的**下载并应用**明确请求经验证的下载、安装及激活。官方和已安装包同 ID 时在各筛选中共用**一张卡片**，不重复；只有真正激活的已安装修订版才显示**活动中**。已安装卡片使用本地肖像，保留选择、检查/诊断、更新/删除、历史恢复；手动导入 ID 与目录匹配并不能证明来源或版本，绝不悄悄覆盖。远程 1024 × 1024 rig 肖像受支持，但编码和解码预算均为 **4 MiB**，包验证上限不变。

点击已安装卡片/修订版**只暂存候选**；查看固定底栏后点**应用**才选中。**取消**清除未提交候选。关任一窗口保留候选，重开浏览器保留搜索/筛选。单独**导入**文件夹/`.herdrchar` 既不暂存也不激活；CLI `pack select ID` 立即选最新修订版、`pack restore ID --revision N` 选已有历史修订版，无需 UI 应用。删除活动包回到 Rubelia。内置 `default@0` 不能导入/更新/删除。关浏览器不取消下载；**取消下载**只能在提交前阻止更改。

包变更进行中，选中/应用/取消/导入/更新/删除/恢复均被阻止，只读检查/诊断仍可用；组合输入期间阻止过期卡片更改；无包变更时取消和检查可用。选择器/确认返回时重新检查忙碌状态与列表代次，不悄悄换基准。包存储锁竞争可暂时显示肖像加载，真实预览失败是错误。忙碌→空闲时已安装卡片的更新/删除/恢复资格刷新，不更换卡片或代次，检查仍可用；取消/失败保留候选。**未提交请求被拒**与**已提交但结果不确定**不同：从底栏提示/诊断读完整操作 ID，用 `pack status OPID` 查询，切勿盲目重复。应用或新候选后诊断仍保留最近操作详情。成功导入/更新/删除非活动包，无需渲染器激活就已完成；角色实时状态与保存的选择不一致会与完成状态分开显示。安装/保存后原生激活仍可能失败，须分别判断。**成功完成的角色包操作摘要卡片隐藏**，诊断仍在；候选、错误、进行中、失败、取消或不确定结果仍显示。无候选时有意义反馈的标题为**操作**，不隐藏已验证的**会话**成功。

包 CLI 使用位置 ID（select/restore 不使用 `--id`），可选操作 ID、预期代次、有限等待/不等待；离线默认变更同步完成，明确异步/有限等待要求已有守护进程，不暗中启动。ACK 不是提交/原生应用，有限等待在 ACK 后只有一个含休眠/状态/解码的绝对期限；不等待却已经终结失败会非零退出。保留 ID 查状态，不重试不确定工作；CLI 不提供创建官方浏览器操作的 URL/目录命令。参阅[包命令、操作状态与安全契约（英文）](cli.md#pack-commands-and-existing-boundaries)，这里不重复命令表；角色制作/模板见[开发文档（英文）](development.md#character-authoring)。

## 观察来源

当前源码展开默认折叠的 **连接 → 观察范围**；公开 v0.3.6 使用 **设置 → 观察来源**。远程仅用于 Auto/Herdr，Tern 专用隐藏远程／设备控件，但保留已保存选择。

远程观察首次出现在 v0.1.6；稳定版 v0.2.0 加入精简观察设置、只复制的重连命令，并修复独立远程模式 watcher 启动。本地**此 Mac**默认开，**远程机器**默认关。远程使用前在**交互式终端**按 [Herdr 保存机器指南（英文）](https://herdr.dev/docs/0.9.2/connecting-machines/)添加并认证 SSH 机器（例如 `herdr machine add workbox`），完成任何被要求的远程服务器安装；再启用远程机器并在**要观察的机器**中明确选择已启用配置。一个配置只观察**一个保存的远程 Herdr 会话**，不是整个主机；注册/attach 本身不会选中。宠物应在显示桌面的**本地 Mac**上运行，兼容的远程 Herdr 服务器**无需**宠物应用/插件；本地 CLI 轮询选中远程配置。

下文的 30 秒退出规避与确认禁用退出只适用于 **Herdr 专用模式**（注册 Herdr 适配器）。Auto／无适配器不需要此规避设置。

远程独立运行且无健康本地服务器时，可在**首次启动前**用已安装原生 CLI 执行 `desktop-pet settings set exit_with_herdr off`，避免配置过程中 30 秒生命周期退出；远程轮询健康后可再开启。该设置不要求远程插件或 Herdr 插件操作。关闭**此 Mac**会排除本地卡片，但**不会**注销本地生命周期端点或覆盖确认禁用后的退出。保存机器目录即使远程关闭也每 **5 秒**刷新；启用的已选配置在远程开启时每 **5 秒**轮询。选择跨重启和远程关闭保留；改名仍保留，禁用/删除配置则排除它而不终止进程。本地与各远程来源即使会话 ID 相同也有独立标签/身份。

行分别显示名称、保存会话和状态；长诊断自动换行、面板可滚动。设置区分首次目录查询与确定为空；刷新失败保留旧列表并提示过期。远程关闭显示**观察已关闭**，不沿用旧成功。状态还区分未选择、暂停、检查中、观察中、不可用、禁用。按不透明配置 ID 建立的控件在轮询/改名时保留焦点/滚动，列表缩短时限制滚动；标签和辅助功能名称本地化；**如何注册**及折叠诊断只读。来源开关/选中配置决定卡片、计数、总体阶段及完成/结果反应；状态卡片筛选只影响展示哪些已纳入卡片。全部来源关闭得到**未知**，而不是被有意排除来源的断线警告。

5 秒轮询可能错过短暂状态变化，包括短暂完成。断线后最后卡片保留为**离线**、计为未知而非在线；重连/重选的第一个新快照是基准，不重放完成。认证时用 `herdr machine list` 查 ID，替换占位符后**自己**在交互式终端运行 `herdr machine reconnect '<profile-id>'`。**显示错误详情**展开只读诊断；**复制重连命令**复制安全引用的命令并反馈，绝不执行。应用不回答凭据、不远程安装，也不为失败远程配置回退本地轮询；远程卡片始终只读。本地观察要求 Herdr ≥0.9.0；远程要求**本地 CLI**支持保存机器 API 转发（`herdr --machine <profile-id> api snapshot`，见 [Herdr 0.9.2 CLI，英文](https://herdr.dev/docs/0.9.2/cli-reference/#saved-ssh-machines)）及兼容的运行中远程服务器。缺少/不兼容 CLI 在设置中显示，不抬高本地版本门槛。`client.attached` 自动启动需要**另外打补丁的宿主**，仅清单最低 0.9.3 不够，见[宿主集成（英文）](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)。

## 生命周期与插件操作

Core 拥有公共 lifecycle 持久化与 `auto_start`（默认 on），额外键由注册集成拥有。Herdr 所有者**仅在 `lifecycle.json` 不存在时**，在首次 lifecycle 写入的共享 profile writer 内原子初始化 `exit_with_herdr:true`。已有文件缺键时读 false，显式 false 保留。旧布尔 `enabled`（含 false）迁移为 `auto_start`。端点、启动元数据、其他所有者字段保留；保存到 `lifecycle.json` 而非 `preferences.json`。坏 JSON／值报错不重置，保存失败保留原策略。Auto／无适配器无需 Herdr 也保持运行；下文 Herdr 端点／断连规则仅适用于 Herdr 专用选择。

自动启动开启时，服务器启动或**成功的 shell/终端客户端 attach** 会启动缺席的宠物；关闭时自动 `ensure` 对缺席宠物报告成功跳过，可登记已有进程的端点，绝不显示已隐藏的宠物。手动 `start` 在自动启动关闭时仍可用，不改变设置。`stop`/原生**退出**结束本次运行（包括待处理启动）；`restart` 手动启动，保留设置/位置/缩放。自动启动仍开启则下次合格事件可重启；想阻止就关闭开关。Herdr 插件禁用/解绑是**独立宿主操作**，不是首选项。原版 Herdr 0.9.0/0.9.3 **没有** `client.attached` 实现，需给官方 0.9.3 安装随附补丁，或使用真正支持该钩子的未来宿主版本。未知钩子的 link 警告不代表成功，按[宿主集成（英文）](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)操作。

在**新**本地端点上，有效 `plugin_list` 缺少 `desktop-pet` 表示未安装，**不是**禁用；仍可观察本地快照/选中远程配置。显式 `enabled: false` 解绑端点。本守护进程一旦在端点观察到已启用**或禁用**的条目，之后缺失表示观察到解绑；该记忆跨检查/重连但新守护进程会清除。**所有已登记本地端点**确认禁用/解绑时**立即**退出，即使远程健康或 `exit_with_herdr` 关闭；最初就缺失不算。否则退出开关开启时，失去**最后一个健康服务器连接**（包括初始无法连接）提供 **30 秒**恢复窗口。客户端 detach 不是服务器断线；过期端点/失败重试不延长窗口。关闭退出开关取消倒计时；断线时再开启会从头计时 30 秒。另有健康连接则保持运行；退出关闭时手动启动的宠物可无 Herdr 运行。切换自动启动不停止/显示当前宠物；`show`/`hide`/`toggle` 仅改可见性。

针对运行且启用的 Herdr 宿主，可用 `herdr plugin action invoke status --plugin desktop-pet`，`settings`、`restart`、`stop` 也采用相同 action-invoke 形式。**当前源码：**无附加参数的原生 `desktop-pet settings` 与现有 Herdr 插件 Settings 操作打开**同一统一设置标签**，不再使用单独的生命周期窗口。宠物停止时，显式 `settings` 以所选配置档保存的宿主启动，同时保留角色／气泡的隐藏状态；源码离线 `settings` 也可接受 `--assets PATH`。请使用匹配的配置／状态路径。原生入口无需 Herdr 宿主，插件操作仍需该宿主。**公开 v0.3.6** 保留旧的独立生命周期设置窗口行为，宠物停止时也可打开。

原生 `settings get`、`settings set auto_start off`、`settings set exit_with_herdr on` 的生命周期专用 CLI 契约不变：get/set 返回结构化 JSON；set 只接受 core `auto_start` 或注册适配器拥有的键与准确的 `on|off`，不启动宠物，拒绝无效输入／保存失败且不重置其他设置；`status` 报告两项。其他 action ID 有生命周期 `start`、`ensure`、`stop`、`restart`、`status`、`settings`；可见性 `show`、`hide`、`toggle`；`passthrough`、`alpha_passthrough`；气泡 `show_bubble`、`hide_bubble`、`bubble_above|below|left|right|auto`；几何 `reset`、`bigger`、`smaller`。[原生自动化契约（英文）](cli.md#english)区分命令式操作、绝对表示／首选项、私有套接字及各操作类别状态恢复；没有公开 `orchestrate` 或 HTTP 服务。

## 应用内更新

**设置 → 应用更新**显示相互独立的自动检查（默认开）和**接收 beta 更新**（默认关）、**立即检查**、运行版本/来源、按策略的最后成功检查时间及明确的来源相关操作。仅稳定版选官方稳定发行；加入 beta 后选合格稳定/beta 中最高 SemVer，同基础版本的正式版优于 beta，退出 beta 不降级。应用打开时自动检查最多每日一次，稳定/含 beta 各有时间戳。改变 beta 策略使原候选失效，仅自动检查开启时按新策略检查。开关和检查不会安装/重启、刷新 Brew 元数据或修改 Git；执行操作需额外原生同意。 检查不改变 lifecycle、安装、运行 image 或所选 profile，但成功检查可写入 `preferences.json` 的策略相关 timestamp 缓存（`last_update_check` / `last_beta_update_check`），并非字节级只读。

- **Standalone：**验证固定中立 release、stage，并在 native installation admission 下原子替换实际账户 home 的应用。共享 locator 提供发现依据，不从 host manifest/socket 父目录推测。可复用已验证所有者；live profile/reservation 阻止危险替换。常规中立更新不调用 Herdr plugin manager；旧所有者 retirement 仅属于原始批准的[迁移路径](migrations/desktop-pet.md)。
- **Homebrew：**保留捕获的管理器/前缀，明确指定目标公式/版本。物理替换范围为跨 keg 的公式 **Cellar 子树**，非整个前缀。确认合格的稳定 ↔ beta 转换占用两处物理安装根，验证刷新的目标元数据并安装/升级目标，不暗中卸载原公式。检查仅读取缓存元数据。
- **本地检出：**明确重建磁盘上当前文件，不执行 Git pull/reset/引用变化；重建结果可执行文件相同也可接受，但替换实例必须确实就绪。
- **已验证的外部安装：**已签名且兼容 protocol-4 的已安装包可提供**应用已安装更新**而不再运行安装器；即使策略仅稳定版，已明确接受的合格安装 beta 也可如此。手动/未知来源不暗中改标为 Herdr 或 Homebrew。

同意和 Prepare 会复查未保存草稿、IME、待处理工作和手势，然后在**原实例确切仍运行**时冻结变更控件。包括对话撤销/重做、浏览器搜索/筛选/本地文件夹操作的 AX/内部回调；安全取消/协调会恢复原生控件状态和编辑历史。助手所有权 **ACK 不是安装、Stop 或 Quit**：只有 Prepare 获接受之后、对应的最终可见/已准备 ACK 才转移同意；只有对应且已预留的 Stop 收到的回复完整交付，才提交更新器关机。慢速暂存在线程中进行，助手 UI 可响应；原生 30 秒期限表示等待，不是取消。回复失败让原实例解冻而不启动安装器；普通用户 Stop/Quit 优先于更新器重启。**助手产生前**已证明失败可在修复和重新检查后重试；助手已产生/所有权不确定则须证据恢复。

独立的**已签名助手**、选择的配置/状态和自定义资源必须位于物理可变安装范围外；另一个运行中的配置共享安装会阻止替换。助手保留所选配置、后端和资源，验证新实例的真实就绪，并区分**已安装**与**已应用**；已应用不等于最新公开版。候选启动必须绑定准确、持久的**镜像与安装来源**组合，而非从当前磁盘重造的 pin。兼容的不完整标记可以修复，不重放管理器；不完整 pin、偏移及不确定管理器执行保持阻止。单独验证的 Original 仅能在持久失败更新目的下恢复，不能报成升级。用户 Stop 禁止复活，但允许协调并释放已完整证明安全停止的事务。已证明的 **NoSpawn** 中断且原实例准确就绪时维持运行/解冻；后来新的成功检查可提供新计划而不抹除历史。绝不自动重放管理器或悄悄删日志。离线 `update-status` 报告保留记录，不证明实时进程/签名；使用验证过的私有助手的 `status` 和明确租约 `recover`，参阅[更新器状态/恢复契约（英文）](cli.md#app-update-status-and-recovery)。

旧 protocol-1/2/3 证据保留并阻止，仅由保留的匹配签名 helper 查询/恢复，不转换成 v4。`updates-v4/` 与实际 OS 账户 home 的 `.desktop-pet-updates` 和旧 `updates/`/`.herdr-updates` 共存。锁顺序为 old global → new global → 排序 installation roots，并检查 legacy stop-generation。**历史** bridge A→neutral B 需要独立同意与已验证 v4 `migrate-legacy --plan ABS`。即将采用的常规路径由原始批准自动完成同一 profile Commit、真实 consumer rebind/native receipt，然后才执行已批准的 retirement。文档与候选 package 不证明公开或验收完成。[迁移与恢复](migrations/desktop-pet.md)。

## 配置与问题排查

- 配置档优先级为 **显式 `--config-dir`/`--state-dir` > `DESKTOP_PET_CONFIG_DIR`/`DESKTOP_PET_STATE_DIR` > default-profile record > 中立 XDG 默认值**（未设置 XDG 时为 `~/.config/desktop-pet`、`~/.local/state/desktop-pet`）。core 不隐式使用 `HERDR_PLUGIN_*`，仅 Herdr wrapper 将宿主注入转换为显式参数。config 保留 lifecycle/preferences、characters、menu-bar-icons；state 保留锁、私有 `control.sock`、daemon log 与更新证据。使用并备份实际匹配的两个目录。`--herdr-socket ABS` 是注册 Herdr 适配器的 backend 选项，不是控制 socket；没有适配器 core 不规范化／注册它。[准确配置档与发现契约](development.md#profiles-and-installation-discovery)。
- 仅注册 Herdr 适配器的 Herdr 专用选择会应用 30 秒断连退出；Auto／无适配器保持运行。若该选择下确实退出，检查同一 profile 的 `settings get`／`status`，重启前设 `exit_with_herdr off`。隐藏窗口不代表 daemon 退出。全部登记本地端点明确禁用／解绑是独立的立即退出，不受此开关覆盖。
- 窗口隐藏或全窗口穿透不等于启动失败，可用上文菜单栏恢复。`plugin link` 后没运行时检查 `client.attached` 警告，手动 `start`，查看守护进程 `status`/日志；`auto_start` 关闭本来就跳过自动启动。链接运行中的宿主不会单独启动宠物。客户端 attach 宿主补丁的隔离部署与钩子确认见[宿主集成（英文）](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)。
- 遇到 `Herdr watchers are shutting down`，检查继承的 `HERDR_SOCKET_PATH` / `HERDR_CLIENT_SOCKET_PATH`、`PATH` 中真实 CLI 可执行文件/版本及拥有套接字的服务器。对**另一服务器**执行 `herdr plugin list` 无法诊断守护进程端点。要针对相同 CLI/套接字/会话；远程错误还需检查保存机器转发/版本/所选配置。补丁宿主提供自动启动，不会在每台服务器安装插件。
- 远程错误参阅[观察来源](#观察来源)：配置状态、交互认证/重连及兼容转发 CLI。GUI 应在本地 Mac 启动，而非 SSH 到远程期待本地窗口。`agent.prompt` 限制和远程只读行为见[上文](#内联回复与消息传递)。
- 不可变 v0.4.7 的默认／`--prebuilt` 使用精确公开固定资产（历史 v0.4.0／v0.4.1 保留各自 pin）。下载／验证失败绝不暗中构建 source。显式 `--source` 构建所选 core 检出，包括实际公开 v0.4.7 标签。 **Core v0.4.7:** [Desktop Pet v0.4.7](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.7), release `413584934`, source `52514da5115f09aac22de5429c08f7acdd993258` (2026-10-15T08:39:17Z). Six anonymous assets, strict/deep signatures, protocol 4 / observation 2 / API 1 and exact 68-file closure passed. Current observed acceptance: [v0.4.7 ledger](releases/v0.4.7.md). Pages is not yet observed; Wiki publication lacks dedicated `WIKI_PUBLISH_TOKEN`.
- 提供方集成由 Herdr 上游负责；安装上游生命周期钩子后重启/重新加载现有提供方会话。宠物仅展示观察到的状态，不推断提供方结果。各版本行为见[发行历史（英文）](releases/README.md)，宿主细节见[宿主集成（英文）](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)。
