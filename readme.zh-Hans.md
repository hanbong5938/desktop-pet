# Desktop Pet

独立的原生 macOS 桌面伙伴。露贝莉娅回应触摸，显示 Herdr、Tern 等已注册提供商的会话，并在来源允许时发送回复。无需宿主也能运行。

[English](readme.md) · [한국어](readme.ko.md) · [日本語](readme.ja.md) · [简体中文](readme.zh-Hans.md)

**当前公开 Core：**[Desktop Pet v0.4.4](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.4)，release `412446874`，发布 `2026-10-14T07:31:12Z`，不可变源码 `b7ab7261aa239441273065e1df6ba7402c750454`。六项匿名公开资产、strict/deep 签名、配对 protocol 4 / capability 2 / API 1、全部 68 个部署文件哈希和精确常规文件 closure 均通过。archive SHA-256 `be59d299cdfc631858239e729186c09951da57811635fea3adeab9e21941695c`；installer SHA-256 `4a2b5d222513cae77045229e62f59e2b4d0c871cab0f1904ee1108be00acef33`。[CI 38528323235](https://github.com/hanbong5938/desktop-pet/actions/runs/38528323235) 的 release job `115733881402` 通过。后续 Wiki job `115735603636` 在实际 Linux 上成功**渲染38个管理页面**，随后以 `WIKI_PUBLISH_TOKEN is missing; use a dedicated credential with Wiki write access` 失败。Wiki 发布仍需所有者初始化 Wiki Home 及专用 Wiki-write secret；不得复用广泛所有者令牌，也不得为修复 Wiki 重跑二进制 Release。最终当前 Pages 部署及四种语言渲染尚未验证。

中立 v0.4.4 配方已公开于 tap `2aa9a4be9517e0232a3f2b8e2b3c748045c69586`。合格 Brew v0.4.4 全新安装通过（74个文件 / 44.3 MB，五项内部/外部重新签名），认证检查确认 Homebrew 派生 image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312`。安装/检查没有创建中立 config/state 或 standalone app，也没有启动守护进程。仅显式手动 **Start** 后才达到实际 v0.4.4 UI/control ready，PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470`。露贝莉娅及实际 Settings 显示 **Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0**；随后显式 Hidden 保留同一默认实例。这是全新安装证明，不是 v0.4.4 常规更新或 managed 迁移完成证明。

**当前公开 bridge：**[Herdr bridge v0.3.9](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.9)，release `412458870`，发布 `2026-10-14T07:47:14Z`，源码 `0f33f2806412fe814367f58bdfbbe2e833399a3e`，tree `1572984289c3eb2049b4d5b17c27b013c2ca593a`。精确的 **11个常规 UTF-8 Git 文件**包括 metadata/downloader/license/readme 及仅狭窄的 `/dist/HerdrDesktopPet.app/` ignore。全部6个 producer scripts/manifest/ignore blob 与公开 Core `b7ab7261aa239441273065e1df6ba7402c750454` 一致；retirement 保持 Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`。五项匿名公开资产与独立验证 CI 字节、strict 签名、配对 protocol 3 / capability 1 / API 1 和精确68文件 hash closure 一致。archive SHA-256 `061dc6cb04e1512db90f0ffe1e9e261fd2be0935924e189ca0c7f8ce3c43808c`。[Bridge CI 38529018457](https://github.com/hanbong5938/desktop-pet/actions/runs/38529018457)，job `115736118653` 通过，包括实际 shipped-native `TMPDIR=/tmp` 及真实 Git admission：生成应用保持 clean，tracked 修改、无关 root 文件及 `dist/user-owned-note` 仍为 dirty。old stable 配方 tap `349e3074ef41960096fbfad884f0ef4b17930e0f` 选择 v0.3.9。冻结 v0.3.6 仍是历史 latest；beta 配方、标签及资产不变。

**实际最终公开 managed A 完成：**冻结 v0.3.6 Check 选择公开 v0.3.9，仅改变 `preferences/last_update_check`（`1791963743` → `1791965310`）；Check 期间其余四棵 tree 字节、source PID `2467` 及无关默认 PID `2641` 不变。精确 source `0f33f28` native A 同意完成 operation `d5b2bf6f02e031eabb5a8c8fdd805cab`（`completed` / `manager_exited`，installed/applied true）。实际公开 v0.3.9 native PID `3417` / instance `b42b25d48315e66bd87e7685e58cfb14`，image `0ed12d159b6bbb83ada833d39b4524ac5f7eccb1253b7acaf9aa2b71cbf833ce` 在精确旧 private config/state 上达到配对 protocol 3 / capability 1 / API 1、UI/control ready、Herdr 1 / Tern 0。实际 managed source Git status 完全 clean。保留的原始 v0.3.6 helper 通过 strict/deep 签名，实际 status 确认 completed。baseline 已保存全部87项旧 A 证据和91个 state 文件哈希。

**实际最终公开 managed B 完成：**单独的 Move preview 与独立 strict/deep 验证通过的公开 v0.4.4 coordinator 收到 native **Migrate** 同意，operation `d729fd740f4e459945edda00bf48d944`、plan SHA-256 `6471e928254229a66a6fc64a88f67095777d6bd890d621feb36b08a4c7d1fc69` 达到 **COMPLETE**。Native Prepare/Stop 停止公开 v0.3.9 PID `3417` / instance `b42b25d48315e66bd87e7685e58cfb14`，选择合格的既有 Brew v0.4.4 app root/image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312`；`target_manager:null` 表示**没有重装**。Target PID `4104` / instance `8912b3aad5419b1c14a78ef3418ae36f` 在精确旧 private config/state 上保留已保存的 Herdr 选择（Herdr 1 / Tern 0），达到 protocol 4 / capability 2 / API 1 及实际 UI/control ready；native_ready 和 target_committed 均为 true。真实 Herdr retirement attempt 1 为 PID = PGID `4140`、exit 0、quiescent true、error null。实际 plugin list 显示不可变 Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8` 的 enabled integration-only Desktop Pet，managed Git HEAD 一致；真实 manager 输出为 v0.4.1、21 actions、startup/client-attached hooks。Managed dist/app 及旧 PID `3417` 消失，未创建 standalone `DesktopPet.app`。精确 retired v0.4.1 标签的历史 Core v0.4.2-minimum wrapper 实际复用同一 v0.4.4 PID/instance，没有新 app 或 tag。全部87项原始 A 证据 SHA-256 及所有既有旧 source-config 哈希不变；无关默认 config/state 两棵 tree 字节相同，隐藏默认 v0.4.4 PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470` 不变且仍运行。仅 adopt private XDG base profile，未与 global default 合并。实际 GUI 已目视确认 Desktop Pet / 内置露贝莉娅及 Settings **Herdr Connected 1 / Tern Disabled 0**；clean-image capture 仍待完成。无 fault 的最终公开 v0.3.9/v0.4.4 A/B happy path，与已单独通过的历史 working metadata `f7edf5e` definite failure/显式 Resume 验证区分。**实际 beta ingress、中立 v0.4.4 常规更新、真实 SDK 两种启动顺序/prompt/transcript/reconnect 及最终当前 Pages 仍待验证；完整 matrix 尚未完成。**

**consumer 与 SDK 边界：**Herdr 集成保持公开 v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`（无资产 release `412252863`）；历史 Core v0.4.2 minimum 冻结不变，健康 Core v0.4.3 复用已有实际观察。当前 Herdr consumer scripts 及全部8项 metadata/evidence 文件现已从公开 Core v0.4.4 release `412446874` / 源码 `b7ab7261aa239441273065e1df6ba7402c750454` 实际 materialize；没有新的 Herdr 标签；最终公开 managed retirement 及不可变 wrapper 对 v0.4.4 的复用已由上文实际 B 结果单独确认。Tern v0.2.0 及兼容 Core v0.4.1 pin 不变。Tern 必须显式 **Configure → Save → Connect** 验证可执行文件及实际 config/state 对；不会安装或自动启动应用。真实 Tern keychain/provider 登录和 genuine OMP 18.10.3 providerSignIn/NoModel 仍阻止两种启动顺序的同实例/prompt/transcript/reconnect 验收。不得复制凭据或伪造账户、ACK、完成结果来替代这些关卡。**完整 matrix 尚未完成。**

**历史 v0.4.2 / v0.3.7 证据（不是 v0.4.4 运行证明）：** **发布边界：**[Desktop Pet v0.4.2](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.2) 已公开（release `412241744`，`2026-10-14T01:09:50Z`，不可变源码 `cb10ce2c3cc65402cbb914d60301f7627d5546d5`）。已验证六项匿名资产、strict/deep 签名、protocol 4 / capability 2 / integration API 1，以及全部 68 个部署资源哈希和精确 closure。tap `32919c745864b54a2e716fcc5a3714d0d38d3e84` 的中立配方及实际公开 v0.4.1 → v0.4.2 Brew 升级证明了 receipt/derivation、五项签名、`brew test` 和 Auto 原生 UI/control ready（Herdr 0 / Tern 0）。全新公开 standalone v0.4.1 安装与两个并发安装器复用运行中所有者已验证；原生 Check 提供实际公开 v0.4.2，经同意完成 `standalone_replace`、`manager_exited`、`installed:true`、`applied:true`，相同 config/state 和精确公开 v0.4.2 image 准备就绪。Check 保留 locator/profile/lifecycle/locks/PID，但改变了 `preferences.json` 检查缓存，不能声称所有配置字节不变。[修正 integration-only Herdr v0.4.1](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.4.1) 已公开（release `412252863`，不可变 ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`）。两个公开 wrapper 保持隐藏并复用相同 Brew v0.4.2 PID/instance/image；显式 config/state 加真实 private patched Herdr 0.9.3 socket 达到 Auto Herdr 1 / Tern 0。全新环境的两个公开 bootstrap 安装/复用一个 OS-home 应用，无集成内 native 文件，也未启动守护进程。这不是公开 Herdr manager 安装 receipt 证明。原始 Herdr v0.4.0 的生成应用/失败测试 scaffold symlink 源码缺陷保留，不能称其为 integration-only。[Bridge v0.3.7](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.7) 已公开（release `412267469`，不可变 tag `987a4b859f95e922c1c5443ead372d10d0dddb24`）。来自精确 Core `cb10ce2` 与修正 Herdr `6d2ac04`，五项匿名资产、签名及 68 文件 closure 已验证。tap `657f32cc4cc563ae40ae0432ac5123dcb3f58426` 只将 old stable 配方更新为 v0.3.7，beta 不变。manual-origin private 真实宿主运行达到 Herdr 1 / Tern 0 ready，但不是 managed A→B 证明。实际冻结公开 Brew v0.3.6 → v0.3.7 A `6a10663c2d2f0177990623a969238af4` 与单独同意的 Core v0.4.2 B `c66961f9e5a91c0023545065ae61385b` 已完成，包括真实 Brew retirement（exit 0、quiescent）。B 前后所有配置文件哈希与旧 A 证据的 87 个文件均不变；实际 Core v0.4.2 UI 显示 Desktop Pet / Herdr Waiting 0 / Tern Disabled 0。相反，真实 managed v0.3.7 A `a1aec5902020de7bcaaf583b1a5f50a5` 在 native `/tmp` 配置目录验证处失败：checkout 替换/registry 注册前 manager exit 1，冻结 helper 保留 Unknown / `manager_exited`、installed/applied false。VM、journal、reservation 保留，未 recovery/replay。

**历史公开 v0.3.8 managed 结果与安全 successor 边界：**managed v0.3.6 → 公开 v0.3.8 A 已完成（`16b6307413ec409efe3eb5a04c9cc21f`），原始签名 v0.3.6 helper 确认 completed status。显式 Check 后 B preflight 以 `managed source changed; migration never resets user changes` 拒绝：`dist/HerdrDesktopPet.app` 的生成文件恰好68个为 untracked。这发生在 B 同意/Stop 前，没有 B manager/fault flag，当时 source PID `3563` 不变。原始公开 v0.3.8 标签恰有10个常规 UTF-8 Git 文件且没有 `.gitignore`；此 B 限制是历史事实，公开 v0.3.9 不会追溯修复旧标签。不要手动 ignore、`git clean` 或 reset 来让旧标签通过。已完成 managed v0.3.8 A 的用户，必须先用保留的**匹配签名 helper**确认已知 terminal 旧 operation，备份实际 config/state 与旧证据，显式 **Stop** 源实例，再由**真实拥有该安装的 Herdr manager**显式安装 successor。绝不 replay Unknown manager operation；保留 VM/journal/reservation 供协调。

**历史 working-metadata 与常规更新证明（不是最终 v0.3.9/v0.4.4 验收）：**真实 managed B 使用 working metadata `f7edf5e0eba374248b9a5ee9d66d26109b39eca2`、不变的公开 v0.3.8 native 和公开 Core v0.4.3，在 NativeReady/Commit 后达到 definite retirement failure（manager exit 1、quiescent）。仅移除引入的自有 `uchg` flag，再以同一 plan SHA `3615fa140fd1b0d49910f5f2ef170b64122243c6c65d0098d4ecfd729e951f16` 获得独立显式 Resume 同意，attempt 2 完成（真实 CLI exit 0、quiescent）。旧 managed app/source 已 retirement，Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8` 复用同一 v0.4.3 instance。全部87项旧 A 证据、5项 first-attempt/plan/consent 文件、旧 config 及无关默认 profile 哈希不变。实际公开中立 v0.4.2 → v0.4.3 常规更新亦已完成（`81c82d837b65fb03584bfb1e668119c3`，`standalone_replace`），同 profile/Herdr 0 ready 及露贝莉娅 UI，无 Herdr 重装。Check 仅改变 `last_update_check`；Check 后全部 config bytes 及87项旧 A 证据在更新前后不变。这不证明最终公开 v0.3.9 A/B 或 v0.4.4 更新。


<img src="assets/rubelia-thumbnail.png" alt="默认桌面伙伴露贝莉娅" width="220">

## 功能

- 可移动、调整大小的桌面角色，支持触摸与抚摸反馈、独立状态气泡和菜单栏控制；界面可跟随系统语言，或选择韩语、英语、简体中文、日语。
- 观察到的 Herdr 会话卡片支持搜索、筛选与排序，并明确区分本地／远程来源；远程观察可选且只读。
- 向选中的**本地**代理会话发送单行回复，支持输入法组合输入与原生状态反馈。
- 可配置角色、气泡、对话和可见性；支持可选的已验证角色包与官方目录浏览器。
- Herdr 生命周期操作、受保护的关联工作树移除，以及明确区分安装来源的应用更新。
- **capability 2：**统一的连接设置可选择 Auto 或已注册提供商（如 Herdr／Tern），显示实际提供商状态，并将运行中的切换保存到共享配置档；当前公开 Desktop Pet v0.4.4 包含此 UI，历史 Herdr v0.3.6 则没有。这不代表双宿主真实账户／提供方运行验证已完成。

## 系统要求

Apple Silicon Mac、macOS 13 或更新版本。源码构建需 Rust/Cargo、Bun、Node.js/npm、Xcode Command Line Tools，预构建无需这些工具。应用为 ad-hoc 签名，未经公证。Herdr 快照需兼容本地服务器（0.9.0+），回复另需 `agent.prompt`。可选 client-attach 自动启动需官方 Herdr 0.9.3 应用[集成仓库的补丁和宿主构建指引](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)，或实际支持钩子的宿主。原版 0.9.0/0.9.3 没有该钩子，插件安装不修改宿主。

## 安装与可选集成

中立 v0.4.4 配方已公开于 tap `2aa9a4be9517e0232a3f2b8e2b3c748045c69586`。合格 Brew v0.4.4 全新安装通过（74个文件 / 44.3 MB，五项内部/外部重新签名），认证检查确认 Homebrew 派生 image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312`。安装/检查没有创建中立 config/state 或 standalone app，也没有启动守护进程。仅显式手动 **Start** 后才达到实际 v0.4.4 UI/control ready，PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470`。露贝莉娅及实际 Settings 显示 **Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0**；随后显式 Hidden 保留同一默认实例。这是全新安装证明，不是 v0.4.4 常规更新或 managed 迁移完成证明。安装命令：

```sh
brew install hanbong5938/tap/desktop-pet
desktop-pet --version
desktop-pet start
desktop-pet status
```

预构建与源码安装在 **core** 检出目录中执行，而不是 Herdr 集成目录。默认仅预构建，下载或验证失败不会暗中回退源码编译。`--source` 明确仅构建当前 core，并使用该目录的 `dist/DesktopPet.app`。

```sh
git clone https://github.com/hanbong5938/desktop-pet.git
cd desktop-pet
# 使用已公开的不可变 v0.4.4 安装器及固定资产：
git checkout --detach v0.4.4
bash scripts/install.sh --prebuilt
# 若选择从同一个公开 v0.4.4 标签安装源码：
# bash scripts/install.sh --source
# Replace with the installer-reported executable and your actual profile paths:
/absolute/reported/desktop-pet --config-dir /absolute/dev/config --state-dir /absolute/dev/state start
```

不可变 v0.4.4 的默认／`--prebuilt` 使用精确公开固定资产（历史 v0.4.0／v0.4.1 保留各自 pin）。下载／验证失败绝不暗中构建 source。显式 `--source` 构建所选 core 检出，包括实际公开 v0.4.4 标签。 standalone prebuilt 使用**实际 OS 账户 home** 的 `Applications/DesktopPet.app`，获准 source 使用 core 的 `dist/DesktopPet.app`。admission 可复用已验证健康现有所有者；请使用安装器报告的执行文件，勿假定 dist 路径。共享 locator 原子保存到账户 home `.local/share/desktop-pet/installation.json`，只是发现证据而非权限。无效显式路径／locator 不回退，执行文件／签名／capability／origin 每次重新验证。Brew 拥有实际 formula／prefix，不暗中接管 live／已预约的安装。

仅安装需要的集成。Herdr 是轻量消费者；Tern 不下载、构建、安装或自动启动原生应用。插件更新与卸载只影响集成，不删除共享应用、配置档或角色。常规应用更新不重新安装 Herdr 插件。

```sh
herdr plugin install hanbong5938/herdr-desktop-pet
tern plugin install https://github.com/hanbong5938/tern-desktop-pet
```

## 配置档、连接与迁移

`--host auto|PROVIDER` 选择 Auto 或已注册的不透明提供商，不是固定 Herdr/Tern 枚举。观察选择优先级为 CLI > `DESKTOP_PET_OBSERVATION_MODE` > 保存的 `observation.host` > 新 Auto。旧 `both`→`auto`、host-less→Herdr 仅用于读迁移。运行中通过 Connection／`preferences set --observation-host auto|PROVIDER` 保存后应用。`--herdr-socket ABS` 是注册 Herdr 适配器的 backend 选项，不是 core 控制 socket，无适配器时不注册。

新配置档默认为 Auto。Connection 显示已注册提供商的实际连接，而非安装检测。提供商 ID 是不透明值，操作权限来自 capability，而非名称前缀或路径。Auto 是同时观察而非回退选择。远程来源只读；Tern 允许回复，不允许删除工作树。接受提交不代表代理完成。

参阅[使用指南](docs/usage.zh-Hans.md)、[准确的配置档与发现契约](docs/development.md#profiles-and-installation-discovery)及 [A→B 迁移](docs/migrations/desktop-pet.md)。把 status / Connection 中的实际可执行文件、config、state 三个值填入 Tern Configure，成功 Save 后再 Connect。现有独立 Tern、beta、自定义配置档保留原地，绝不合并。

```sh
desktop-pet profile show --json
desktop-pet start
# Only with the registered Herdr adapter; use its verified backend:
# desktop-pet --herdr-socket /actual/path/to/herdr-api.sock start
desktop-pet preferences set --observation-host auto
```

## 操作

| 操作 | 结果 |
| --- | --- |
| 轻点头部／身体；在头部滑动 | 触发反应；抚摸露贝莉娅。 |
| 拖动角色；使用右下角把手 | 移动角色；调整角色大小。 |
| 拖动气泡背景或边缘／角落 | 移动所附角色或独立气泡；独立调整气泡大小。 |
| 左键点击菜单栏图标 | 打开角色、观察、气泡及生命周期设置。 |
| 展开气泡，选择本地代理卡片 | 打开单行回复；Enter、Command+Enter 或发送按钮提交。远程卡片只读。 |

输入法组合输入期间**绝不提交**；Escape 首先取消组合输入。在搜索框按 Enter／Command+Enter 不会发送回复。角色与气泡的可见性互相独立；两者都隐藏或启用整窗点击穿透后，可能无法通过桌面点击找回。必要时使用菜单栏图标的**显示并启用角色**恢复操作（默认模式始终显示图标；仅恢复模式在必要时显示）。完整的操作、搜索、通过已保存机器进行远程观察和可见性行为，参阅[使用指南](docs/usage.zh-Hans.md#操作)。

## 可选角色和服装

[角色图库](https://hanbong5938.github.io/herdr-characters/)展示可选角色包；[独立仓库](https://github.com/hanbong5938/herdr-characters)保存源码与使用条款。在设置中选择**角色 → 浏览角色…**；浏览肖像不会下载归档。符合条件的已发布官方角色包提供明确、经过验证的**下载并应用**。对于本地文件夹或 `.herdrchar`，仅**导入**不会激活：还需选择已安装卡片／修订版本，再点击**应用**。只有露贝莉娅随应用附带且为默认角色。参阅[角色包](docs/usage.zh-Hans.md#角色包)及[制作说明（英语）](docs/development.md#character-authoring)。

## 安全与限制

- 回复需要所选来源的**本地 prompt capability**。Herdr 使用 `agent.prompt`，Tern 使用单次 `agents:ask`，不绕过远程、SSH、shell、PTY 输入。接受 ACK **不证明模型完成**，Tern 可能仍在队列中。批准／问题在实际终端处理，不确定送达请先核查，勿盲目重发。Herdr 提交 API 存在当前 occupant 竞态，不保证原子固定会话。[送达详情](docs/usage.zh-Hans.md#内联回复与消息传递)。
- **移除工作树…**不可撤销：继续前确认**准确的检出路径**和**整个工作区**的所有标签页／窗格。该操作会终止其中的进程／代理并移除关联检出，**包括被忽略的文件**；不会进入废纸篓，无法撤销。主仓库和分支不会删除。参阅[移除防护措施](docs/usage.zh-Hans.md#删除工作树)。
- 远程观察需要选定已保存的机器配置及兼容的远程 Herdr 服务器，**不要求**在远端安装桌面角色。本地应用轮询远端状态，无法恢复短暂遗漏的状态转换，也不能发送远程提示。参阅[观察指南](docs/usage.zh-Hans.md#观察来源)。
- 自动检查应用更新保留 lifecycle、安装、image、所选 profile，但可写入 `preferences.json` 的成功检查 timestamp 缓存；安装或应用更新需要明确同意。更新器所有权 ACK **不是完成信号**。结果不明时，重试前请查阅[应用内更新恢复](docs/usage.zh-Hans.md#应用内更新)及 [CLI 更新状态与恢复（英语）](docs/cli.md#app-update-status-and-recovery)。

## 配置与故障排查

配置档优先级为 **显式 `--config-dir`/`--state-dir` > `DESKTOP_PET_CONFIG_DIR`/`DESKTOP_PET_STATE_DIR` > default-profile record > 中立 XDG 默认值**（未设置 XDG 时为 `~/.config/desktop-pet`、`~/.local/state/desktop-pet`）。core 不隐式使用 `HERDR_PLUGIN_*`，仅 Herdr wrapper 将宿主注入转换为显式参数。config 保留 lifecycle/preferences、characters、menu-bar-icons；state 保留锁、私有 `control.sock`、daemon log 与更新证据。使用并备份实际匹配的两个目录。`--herdr-socket ABS` 是注册 Herdr 适配器的 backend 选项，不是控制 socket；没有适配器 core 不规范化／注册它。[准确配置档与发现契约](development.md#profiles-and-installation-discovery)。

Core 拥有公共 lifecycle 持久化与 `auto_start`（默认 on），额外键由注册集成拥有。Herdr 所有者**仅在 `lifecycle.json` 不存在时**，在首次 lifecycle 写入的共享 profile writer 内原子初始化 `exit_with_herdr:true`。已有文件缺键时读 false，显式 false 保留。旧布尔 `enabled`（含 false）迁移为 `auto_start`。端点、启动元数据、其他所有者字段保留；保存到 `lifecycle.json` 而非 `preferences.json`。坏 JSON／值报错不重置，保存失败保留原策略。Auto／无适配器无需 Herdr 也保持运行；下文 Herdr 端点／断连规则仅适用于 Herdr 专用选择。

## 文档与网站

[Desktop Pet Wiki](https://github.com/hanbong5938/desktop-pet/wiki) 是 core 维护的生成阅读视图，以审阅后的源码为原本。实际 Wiki 发布需另行验证。旧 Herdr 网站仅是集成与迁移入口，不复制共同应用网站。

- [使用与恢复](docs/usage.zh-Hans.md) · [开发与安装（英语）](docs/development.md) · [原生自动化 CLI 契约（英语）](docs/cli.md#english) · [打补丁的主程序集成（英语）](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)。
- [发行历史（英语）](docs/releases/README.md) · [v0.3.6 变更（英语／韩语）](docs/releases/v0.3.6.md) · [v0.3 升级指南（英语）](docs/releases/0.3.md#upgrade-from-v021-or-beta) · [历史测试版／预览版指南（英语）](docs/migrations/unreleased.md) · [v0.1.11 → v0.2.0 迁移（英语）](docs/migrations/v0.1.11-to-v0.2.0.md)。
- 宣传网站提供[简体中文](https://hanbong5938.github.io/desktop-pet/zh-Hans/)、[韩语](https://hanbong5938.github.io/desktop-pet/)、[英语](https://hanbong5938.github.io/desktop-pet/en/)和[日语](https://hanbong5938.github.io/desktop-pet/ja/)版本，介绍的是此**原生**应用；应用不能在浏览器中运行。中立 Pages 部署尚未在此验证。网站开发／部署参阅 [GitHub Pages 指南（英语）](docs/development.md#website-github-pages)。独立的[角色图库](https://hanbong5938.github.io/herdr-characters/)展示可选角色包，仅提供英语和韩语。

## 美术与许可

[软件许可](LICENSE.txt)与美术／模型条款**彼此独立**：MIT 软件许可并不重新授权角色美术。内置露贝莉娅的[许可](assets/rubelia-default/LICENSE.txt)、[署名](assets/rubelia-default/ATTRIBUTION.txt)、[来源记录](assets/rubelia-default/source-record.json)及[背景修复记录](assets/rubelia-default/background-repair.json)具有权威性。`LICENSE.txt` 还保留原 Coding Cat 声明；其美术／源码已移至[角色仓库](https://github.com/hanbong5938/herdr-characters)。请保留每个可选角色包自己的条款。美术所有者的批准不授予模型权利；Qwen 的研究许可仅为制作记录，应用未附带模型引擎／权重。参阅[角色制作与许可（英语）](docs/development.md#character-authoring)。
