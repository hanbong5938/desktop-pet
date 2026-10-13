# Desktop Pet — Herdr integration

[English](readme.md) · [한국어](readme.ko.md) · [日本語](readme.ja.md) · [简体中文](readme.zh-Hans.md)

**发布边界：**旧版 [Herdr v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6) 已发布，采用 protocol 3 / observation capability 1。独立 Desktop Pet v0.4.0（protocol 4 / capability 2 / integration API 1）与迁移 bridge v0.3.7 是经过审阅的候选版本，公开安装与运行验证尚未完成。以下是中立产品切换指引；文档、标签、源码版本号都不证明已发布。

仅安装需要的集成。Herdr 是轻量消费者；Tern 不下载、构建、安装或自动启动原生应用。插件更新与卸载只影响集成，不删除共享应用、配置档或角色。常规应用更新不重新安装 Herdr 插件。

[Desktop Pet / core app](https://github.com/hanbong5938/desktop-pet/blob/main/readme.zh-Hans.md) · [Herdr host / patch / OMP / link](integrations/herdr/README.md) · [Legacy A → B migration](https://github.com/hanbong5938/desktop-pet/blob/main/docs/migrations/desktop-pet.md)

中立发布及配方实际公开之后，公开安装命令为：

```sh
brew install hanbong5938/tap/desktop-pet
herdr plugin install hanbong5938/herdr-desktop-pet
herdr plugin action invoke start --plugin desktop-pet
```

Apple Silicon Mac、macOS 13 或更新版本。源码构建需 Rust/Cargo、Bun、Node.js/npm、Xcode Command Line Tools，预构建无需这些工具。应用为 ad-hoc 签名，未经公证。Herdr 快照需兼容本地服务器（0.9.0+），回复另需 `agent.prompt`。可选 client-attach 自动启动需官方 Herdr 0.9.3 应用[集成仓库的补丁和宿主构建指引](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)，或实际支持钩子的宿主。原版 0.9.0/0.9.3 没有该钩子，插件安装不修改宿主。

预构建与源码安装在 **core** 检出目录中执行，而不是 Herdr 集成目录。默认仅预构建，下载或验证失败不会暗中回退源码编译。`--source` 明确仅构建当前 core，并使用该目录的 `dist/DesktopPet.app`。

> Native app features below are owned and documented by the independent core, not built in this integration checkout.

## 功能

- 可移动、调整大小的桌面角色，支持触摸与抚摸反馈、独立状态气泡和菜单栏控制；界面可跟随系统语言，或选择韩语、英语、简体中文、日语。
- 观察到的 Herdr 会话卡片支持搜索、筛选与排序，并明确区分本地／远程来源；远程观察可选且只读。
- 向选中的**本地**代理会话发送单行回复，支持输入法组合输入与原生状态反馈。
- 可配置角色、气泡、对话和可见性；支持可选的已验证角色包与官方目录浏览器。
- Herdr 生命周期操作、受保护的关联工作树移除，以及明确区分安装来源的应用更新。
- **仅当前源码：**统一的连接设置可选择 Auto 或已注册提供商（如 Herdr／Tern），显示实际提供商状态，并将运行中的切换保存到共享配置档；公开 v0.3.6 不包含此 capability-2 UI。

### Tern 观察：仅源码 exchange v2

Auto 选中 Tern 或显式选择 Tern 时，当前 exchange-v2 源码自动观察所有已连接的 Tern SDK 主机，而不只是本地代理。卡片显示主机名和本地／远程标签；每台主机的 Offline 状态与观察生命周期相互独立。Tern 主机观察独立于 Herdr 的 local/remote/machines 范围设置，无需新增主机选择界面。回复仍仅限本地，远程卡片不能发送回复。

这不是公开 v0.3.6 的行为，其历史上的仅本地 Tern 路径保持不变。真实远程 SDK 代理及配套的原生应用／插件安装**尚未验证**。当前插件对中立 `DesktopPet.app` 的安装要求并不能证明与此源码分支的 `HerdrDesktopPet.app` 包兼容。完整源码约定与验证边界见[使用指南](docs/usage.zh-Hans.md)和[开发 / 验证证据](docs/development.md)。


## Source / link

Install or build the core first (`bash scripts/install.sh --source` in the **desktop-pet core** checkout). Then link **this integration checkout**, not the core. `plugin link` does not run a build. The integration wrapper discovers a verified compatible native executable, preserving the exact profile/socket provenance; it never compiles a native app here. Client-attach startup remains optional and requires the actual patched host/hook; plugin install does not patch Herdr. See [separate checkouts](integrations/herdr/README.md#build-in-separate-checkouts).

## Ownership

This repository owns `native-adapter`, `herdr-plugin.toml`, thin install/run wrappers, `integrations/herdr` and `integrations/omp`, integration validation, and frozen historical releases. App/UI/rig/artwork/packaging/source instructions belong to [desktop-pet](https://github.com/hanbong5938/desktop-pet). `.herdrchar`, pack v5, `default@0`, rig ABI and [herdr-characters](https://github.com/hanbong5938/herdr-characters) catalog/gallery authority are unchanged. Historical [v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6) and older release bodies/tags/assets remain immutable; an integration-only v0.4.0 does not carry a native archive.

## 美术与许可

[软件许可](LICENSE.txt)与美术／模型条款**彼此独立**：MIT 软件许可并不重新授权角色美术。内置露贝莉娅的[许可](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/LICENSE.txt)、[署名](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/ATTRIBUTION.txt)、[来源记录](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/source-record.json)及[背景修复记录](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/background-repair.json)具有权威性。`LICENSE.txt` 还保留原 Coding Cat 声明；其美术／源码已移至[角色仓库](https://github.com/hanbong5938/herdr-characters)。请保留每个可选角色包自己的条款。美术所有者的批准不授予模型权利；Qwen 的研究许可仅为制作记录，应用未附带模型引擎／权重。参阅[角色制作与许可（英语）](https://github.com/hanbong5938/desktop-pet/blob/main/docs/development.md#character-authoring)。
