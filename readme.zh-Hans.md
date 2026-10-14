# Desktop Pet

独立的原生 macOS 桌面伙伴。露贝莉娅回应触摸，显示 Herdr、Tern 等已注册提供商的会话，并在来源允许时发送回复。无需宿主也能运行。

[English](readme.md) · [한국어](readme.ko.md) · [日本語](readme.ja.md) · [简体中文](readme.zh-Hans.md)

**当前公开 Core：**[Desktop Pet v0.4.3](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.3)，release `412334201`，发布 `2026-10-14T04:20:48Z`，不可变源码 `9bba2e71a1d5a1bcef5f71509ac6701b61b3d674`。已验证六项匿名公开资产、strict/deep 签名、配对 protocol 4 / capability 2 / API 1、全部 68 个部署文件哈希和精确常规文件 closure。archive SHA-256 `b1bdf3b118876f5b705617b30dc321dd9cb1ff71cff147bcd19314e072ea746a`；installer SHA-256 `7d5094f89a7433759ac102f5028217f057d3bbbfa70a0a6f686bbc5a7559f576`。[CI 38510441950](https://github.com/hanbong5938/desktop-pet/actions/runs/38510441950) 的 release job `115677463431` 成功，只有后续 Wiki job 失败：实际 37 页渲染通过，但缺少专用 `WIKI_PUBLISH_TOKEN`，并未发布 Wiki，不复用广泛所有者令牌。中立 v0.4.3 配方已公开于 tap `6a9ae670cb3c69eb6e66bfd99ef351c513168f72`。一次性 VM 中的合格 Brew v0.4.3 全新安装通过；实际原生 GUI 显示 Auto / Herdr Waiting 0 / Tern Waiting 0、PID `2967`、instance `d62441eca0603028793fa846aa0555cd`、派生 image `7b3613635d8cb5e6c26e2b4f4259aa4eeb87bf8eb17c21c7219236e92631c4ae`。显式 hide 保留同一独立默认配置档。[Bridge v0.3.8](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.8) 已公开：release `412343376`、发布 `2026-10-14T04:40:30Z`、仅十个常规 UTF-8 Git 文件的 metadata-only 源码 `e615ed310202d99d4389fb80d7cfc29db87275c6`、retirement ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`。五项匿名公开资产与已验证 CI 字节一致；archive SHA-256 `fc1d69dab9e9287dcb9903bc262b1b2e0ba06690c1b944230b81439b4ff09003`、rig SHA-256 `54ea2e40171407e243726697e6a56eff226244a32f33318ab5d45fd2c8fdda9b`，配对 protocol 3 / observation 1 / API 1、签名及全部 68 项 closure 哈希通过。CI `38511259163` / job `115680064263` 在 `TMPDIR=/tmp` 下通过实际出货 ingress 的 native Rubelia 验证，两个 ingress scratch 目录均为 `/private/tmp`。old stable 配方 `49fa8a2` 选择 v0.3.8，beta 配方与冻结 v0.3.6 资产/标签不变。新的真实 managed v0.3.6 → 公开 v0.3.8 A 已完成（`16b6307413ec409efe3eb5a04c9cc21f`、`completed` / `manager_exited`、installed/applied true），由原始签名 v0.3.6 helper 确认。bridge PID `3563` / instance `9aa35111b6b8e54dfca787b01e3425d7` 以精确 image `be4858803ceec46a9d3ac80cefb2c7d2da0013f1e2e9df49ca99fca01ae675a8` 在相同旧 private config/state 上达到 Herdr mode / Herdr 1 / Tern 0、UI/control ready。这验证公开 managed A 的 `/tmp` 修正，不是恢复保留的失败 VM。Core v0.4.3 managed B、显式 failed-retirement resume、实际 v0.3.8 固定 v0.3.6 beta 路径及中立 v0.4.2 → v0.4.3 更新仍待验证。已发布 Herdr v0.4.1 保留兼容 Core v0.4.2 pin；current-main Core v0.4.3 消费者工作不代表新 Herdr 发布。**历史 v0.4.2 / v0.3.7 证据（不是 v0.4.3 运行证明）：** **发布边界：**[Desktop Pet v0.4.2](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.2) 已公开（release `412241744`，`2026-10-14T01:09:50Z`，不可变源码 `cb10ce2c3cc65402cbb914d60301f7627d5546d5`）。已验证六项匿名资产、strict/deep 签名、protocol 4 / capability 2 / integration API 1，以及全部 68 个部署资源哈希和精确 closure。tap `32919c745864b54a2e716fcc5a3714d0d38d3e84` 的中立配方及实际公开 v0.4.1 → v0.4.2 Brew 升级证明了 receipt/derivation、五项签名、`brew test` 和 Auto 原生 UI/control ready（Herdr 0 / Tern 0）。全新公开 standalone v0.4.1 安装与两个并发安装器复用运行中所有者已验证；原生 Check 提供实际公开 v0.4.2，经同意完成 `standalone_replace`、`manager_exited`、`installed:true`、`applied:true`，相同 config/state 和精确公开 v0.4.2 image 准备就绪。Check 保留 locator/profile/lifecycle/locks/PID，但改变了 `preferences.json` 检查缓存，不能声称所有配置字节不变。[修正 integration-only Herdr v0.4.1](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.4.1) 已公开（release `412252863`，不可变 ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`）。两个公开 wrapper 保持隐藏并复用相同 Brew v0.4.2 PID/instance/image；显式 config/state 加真实 private patched Herdr 0.9.3 socket 达到 Auto Herdr 1 / Tern 0。全新环境的两个公开 bootstrap 安装/复用一个 OS-home 应用，无集成内 native 文件，也未启动守护进程。这不是公开 Herdr manager 安装 receipt 证明。原始 Herdr v0.4.0 的生成应用/失败测试 scaffold symlink 源码缺陷保留，不能称其为 integration-only。[Bridge v0.3.7](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.7) 已公开（release `412267469`，不可变 tag `987a4b859f95e922c1c5443ead372d10d0dddb24`）。来自精确 Core `cb10ce2` 与修正 Herdr `6d2ac04`，五项匿名资产、签名及 68 文件 closure 已验证。tap `657f32cc4cc563ae40ae0432ac5123dcb3f58426` 只将 old stable 配方更新为 v0.3.7，beta 不变。manual-origin private 真实宿主运行达到 Herdr 1 / Tern 0 ready，但不是 managed A→B 证明。实际冻结公开 Brew v0.3.6 → v0.3.7 A `6a10663c2d2f0177990623a969238af4` 与单独同意的 Core v0.4.2 B `c66961f9e5a91c0023545065ae61385b` 已完成，包括真实 Brew retirement（exit 0、quiescent）。B 前后所有配置文件哈希与旧 A 证据的 87 个文件均不变；实际 Core v0.4.2 UI 显示 Desktop Pet / Herdr Waiting 0 / Tern Disabled 0。相反，真实 managed v0.3.7 A `a1aec5902020de7bcaaf583b1a5f50a5` 在 native `/tmp` 配置目录验证处失败：checkout 替换/registry 注册前 manager exit 1，冻结 helper 保留 Unknown / `manager_exited`、installed/applied false。VM、journal、reservation 保留，未 recovery/replay。并非完整 matrix 已通过：显式 definite-failed-retirement resume、真实 SDK 两种启动顺序的同实例/重连及账户验收仍未证明。Core v0.4.3 与 bridge v0.3.8 已公开；Brew v0.4.3 全新安装及新 managed A 已完成，managed B/resume 验收仍待完成。 [Core v0.4.3 / bridge v0.3.8](https://github.com/hanbong5938/desktop-pet/blob/main/docs/releases/v0.4.3.md). Tern v0.2.0 及兼容 Core v0.4.1 pin 不变；真实 Tern keychain/provider 登录与 OMP providerSignIn/NoModel 仍是前提。历史标签、正文、资产与源码保留。Core Pages `cb10ce2` 部署和 EN/KO 浏览器验证成功，但 JA/ZH 视觉验证及最终当前文档部署待完成。Linux Wiki catalog 渲染通过，但发布需要专用 `WIKI_PUBLISH_TOKEN` 及所有者初始化 Wiki Home。Wiki 链接不是发布证明；二进制 release job 成功与后续 Wiki 失败是两回事。

**公开 managed B 的已知限制；修正待完成：**实际 managed 0.3.6 → 公开 bridge 0.3.8 A 已完成（`16b6307413ec409efe3eb5a04c9cc21f`），原始 0.3.6 helper 的签名及 completed status 验证通过。显式 Check 结束 A 的旧 terminal 观察后 Move 已启用，但 B preflight 以 `managed source changed; migration never resets user changes` 拒绝。实际 Git status 仅有 `dist/HerdrDesktopPet.app` 下恰好68个生成文件 untracked，无其他变更。这发生在 B 同意及 Stop 之前；没有 B manager 或 fault flag，原始 PID 3563 保持不变且仍运行。公开 Core 0.4.3 / bridge 0.3.8 仍是当前版本。已批准的新 Core 0.4.4 + genuine bridge 0.3.9 尚待完成：producer 修正仅在 `.gitignore` 声明 `/dist/HerdrDesktopPet.app/`，真实 materialized Git admission gate 将确认生成应用保持 clean，而 tracked script 修改、无关 root 文件及 `dist/user-owned-note` 仍为 dirty。native Git dirty 保护不变。不要用 `git clean`、reset 或手动 ignore 让已发布 0.3.8 通过。原始 0.3.8 标签恰有10个常规 UTF-8 Git 文件且没有 `.gitignore`；新的修正 metadata 将有11个。不声称新版本已发布、managed B/resume 或最终四语言 Pages 已完成；保留旧 0.3.7 Unknown/no-replay 历史及 SDK/Wiki 阻碍。


<img src="assets/rubelia-thumbnail.png" alt="默认桌面伙伴露贝莉娅" width="220">

## 功能

- 可移动、调整大小的桌面角色，支持触摸与抚摸反馈、独立状态气泡和菜单栏控制；界面可跟随系统语言，或选择韩语、英语、简体中文、日语。
- 观察到的 Herdr 会话卡片支持搜索、筛选与排序，并明确区分本地／远程来源；远程观察可选且只读。
- 向选中的**本地**代理会话发送单行回复，支持输入法组合输入与原生状态反馈。
- 可配置角色、气泡、对话和可见性；支持可选的已验证角色包与官方目录浏览器。
- Herdr 生命周期操作、受保护的关联工作树移除，以及明确区分安装来源的应用更新。
- **capability 2：**统一的连接设置可选择 Auto 或已注册提供商（如 Herdr／Tern），显示实际提供商状态，并将运行中的切换保存到共享配置档；当前公开 Desktop Pet v0.4.3 包含此 UI，历史 Herdr v0.3.6 则没有。这不代表双宿主真实账户／提供方运行验证已完成。

## 系统要求

Apple Silicon Mac、macOS 13 或更新版本。源码构建需 Rust/Cargo、Bun、Node.js/npm、Xcode Command Line Tools，预构建无需这些工具。应用为 ad-hoc 签名，未经公证。Herdr 快照需兼容本地服务器（0.9.0+），回复另需 `agent.prompt`。可选 client-attach 自动启动需官方 Herdr 0.9.3 应用[集成仓库的补丁和宿主构建指引](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)，或实际支持钩子的宿主。原版 0.9.0/0.9.3 没有该钩子，插件安装不修改宿主。

## 安装与可选集成

中立 v0.4.3 配方已发布于 tap `6a9ae670cb3c69eb6e66bfd99ef351c513168f72`；实际 v0.4.3 Brew 全新安装/运行仍待验证。 安装命令：

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
# 使用已公开的不可变 v0.4.3 安装器及固定资产：
git checkout --detach v0.4.3
bash scripts/install.sh --prebuilt
# 若选择从同一个公开 v0.4.3 标签安装源码：
# bash scripts/install.sh --source
# Replace with the installer-reported executable and your actual profile paths:
/absolute/reported/desktop-pet --config-dir /absolute/dev/config --state-dir /absolute/dev/state start
```

不可变 v0.4.3 的默认／`--prebuilt` 使用精确公开固定资产（历史 v0.4.0／v0.4.1 保留各自 pin）。下载／验证失败绝不暗中构建 source。显式 `--source` 构建所选 core 检出，包括实际公开 v0.4.3 标签。 standalone prebuilt 使用**实际 OS 账户 home** 的 `Applications/DesktopPet.app`，获准 source 使用 core 的 `dist/DesktopPet.app`。admission 可复用已验证健康现有所有者；请使用安装器报告的执行文件，勿假定 dist 路径。共享 locator 原子保存到账户 home `.local/share/desktop-pet/installation.json`，只是发现证据而非权限。无效显式路径／locator 不回退，执行文件／签名／capability／origin 每次重新验证。Brew 拥有实际 formula／prefix，不暗中接管 live／已预约的安装。

仅安装需要的集成。Herdr 是轻量消费者；Tern 不下载、构建、安装或自动启动原生应用。插件更新与卸载只影响集成，不删除共享应用、配置档或角色。常规应用更新不重新安装 Herdr 插件。

```sh
herdr plugin install hanbong5938/herdr-desktop-pet
tern plugin install https://github.com/hanbong5938/tern-desktop-pet
```

## 配置档、连接与迁移

`--host auto|PROVIDER` 选择 Auto 或已注册的不透明提供商，不是固定 Herdr/Tern 枚举。观察选择优先级为 CLI > `DESKTOP_PET_OBSERVATION_MODE` > 保存的 `observation.host` > 新 Auto。旧 `both`→`auto`、host-less→Herdr 仅用于读迁移。运行中通过 Connection／`preferences set --observation-host auto|PROVIDER` 保存后应用。`--herdr-socket ABS` 是注册 Herdr 适配器的 backend 选项，不是 core 控制 socket，无适配器时不注册。

新配置档默认为 Auto。Connection 显示已注册提供商的实际连接，而非安装检测。提供商 ID 是不透明值，操作权限来自 capability，而非名称前缀或路径。Auto 是同时观察而非回退选择。远程来源只读；Tern 允许回复，不允许删除工作树。接受提交不代表代理完成。

参阅[使用指南](docs/usage.zh-Hans.md)、[准确的配置档与发现契约](docs/development.md#profiles-and-installation-discovery)及 [A→B 迁移](docs/migrations/desktop-pet.md)。把 status / Connection 中的实际可执行文件、config、state 三个值填入 Tern Configure，再 Connect。现有独立 Tern、beta、自定义配置档保留原地，绝不合并。

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
