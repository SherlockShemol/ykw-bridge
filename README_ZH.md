<div align="center">

# You Know Who Bridge (YKW Bridge)

### Claude Code + Claude Desktop 专版管理工具

[![Version](https://img.shields.io/github/v/release/SherlockShemol/ykw-bridge?color=blue&label=version)](https://github.com/SherlockShemol/ykw-bridge/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](https://github.com/SherlockShemol/ykw-bridge/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![Downloads](https://img.shields.io/github/downloads/SherlockShemol/ykw-bridge/total)](https://github.com/SherlockShemol/ykw-bridge/releases/latest)

[English](README.md) | 中文 | [日本語](README_JA.md) | [Releases](https://github.com/SherlockShemol/ykw-bridge/releases)

</div>

## 致谢

YKW Bridge 的起点来自开源项目 [**CC Switch**](https://github.com/farion1231/cc-switch)。

感谢原项目作者 [**Jason Young**](https://github.com/farion1231) 以及所有贡献者完成了这项出色的工作，也感谢他们为后续使用者和开发者留下了一个扎实、可靠的基础。这个项目今天能够继续往前走，离不开原项目最初的投入与积累。

本项目在原项目所采用的 **MIT License** 下继续进行修改、维护与发布，并保留相应的版权与许可信息。我们尊重原项目及其贡献者，也感谢开源社区所提供的共享基础。

之所以继续演化出这个独立版本，一方面是为了更贴合我们自己的使用习惯、维护方向和实际需求，另一方面也是希望把这些调整分享给有类似需求的人。

如果你喜欢这个项目，也欢迎同时关注原项目 [CC Switch](https://github.com/farion1231/cc-switch) 和原作者 [Jason Young](https://github.com/farion1231)。

本仓库为独立维护版本，后续会按照自己的节奏继续演化。


## 为什么选择 YKW Bridge？

现代 Claude 工作流依然要处理 provider 配置、代理、MCP、Prompts、Skills，以及 Claude Desktop 额外的证书、启动链路和接管状态。手动改 JSON 很容易把环境越改越乱。

**YKW Bridge** 现在收敛为 **Claude Code + Claude Desktop** 专版。你可以在同一个应用里管理 Claude 侧 provider、托管认证、MCP、Prompts、Skills、Sessions、Usage，以及 Claude Desktop 的 doctor / certificate / launch 工具链。

- **一个应用，专注 Claude** — 在单一界面中管理 Claude Code 与 Claude Desktop
- **告别手动编辑** — 可视化导入 provider，快速切换，不再手改配置文件
- **统一 MCP / Skills 管理** — 只保留 Claude 路径，减少心智负担
- **系统托盘快速切换** — 从托盘菜单即时切换供应商，无需打开完整应用
- **云同步** — 通过 Dropbox、OneDrive、iCloud 或 WebDAV 服务器在不同设备之间同步供应商数据
- **跨平台** — 基于 Tauri 2 构建的原生桌面应用，支持 Windows、macOS 和 Linux
- **小工具** - 内置了多种小工具来解决首次安装登录确认、禁止签名、插件拓展同步等多种功能

## 界面预览

|                  主界面                   |                  添加供应商                  |
| :---------------------------------------: | :------------------------------------------: |
| ![主界面](assets/screenshots/main-zh.png) | ![添加供应商](assets/screenshots/add-zh.png) |

## 功能特性

[GitHub Releases](https://github.com/SherlockShemol/ykw-bridge/releases)

### 供应商管理

- **Claude 供应商管理** — 一键导入、切换、排序、导出 Claude provider
- **托管认证保留** — 继续支持 GitHub Copilot OAuth 与 ChatGPT / OpenAI OAuth
- 一键切换、系统托盘快速访问、拖拽排序、导入导出

### 代理与故障转移

- **本地代理热切换** — 格式转换、自动故障转移、熔断器、供应商健康监控和整流器
- **应用级代理接管** — 分别接管 Claude Code 与 Claude Desktop

### MCP、Prompts 与 Skills

- **统一 MCP 面板** — 管理 Claude MCP，支持 Deep Link 导入
- **Prompts** — Claude Markdown 提示词编辑与回填保护
- **Skills** — 从 GitHub 仓库或 ZIP 文件一键安装，自定义仓库管理，支持软连接和文件复制

### 用量与成本追踪

- **用量仪表盘** — 跨供应商追踪支出、请求数和 Token 用量，趋势图表、详细请求日志和自定义模型定价

### 会话管理器与工作区

- 浏览、搜索和恢复 Claude 会话历史
- **Claude Desktop 工具链** — doctor、证书、启动 shim、watchdog、代理接管集中管理

### 系统与平台

- **云同步** — 自定义配置目录（Dropbox、OneDrive、iCloud、坚果云、NAS）及 WebDAV 服务器同步
- **Deep Link** (`ykwbridge://`) — 通过 URL 一键导入供应商、MCP 服务器、提示词和技能
- 深色 / 浅色 / 跟随系统主题、开机自启、自动更新、原子写入、自动备份、国际化（中/英/日）

## 常见问题

<details>
<summary><strong>YKW Bridge 现在支持哪些工具？</strong></summary>

当前专版只支持 **Claude Code** 和 **Claude Desktop**。`GitHub Copilot` 与 `ChatGPT / OpenAI` 认证会继续保留，但它们只作为 Claude provider 的托管认证方式出现。

</details>

<details>
<summary><strong>切换供应商后需要重启终端吗？</strong></summary>

大多数工具需要重启终端或 CLI 工具才能使更改生效。例外的是 **Claude Code**，它目前支持供应商数据的热切换，无需重启。

</details>

<details>
<summary><strong>切换供应商之后我的插件配置怎么不见了？</strong></summary>

YKW Bridge 使用“通用配置片段”功能，在不同的供应商之间传递 Key 和请求地址之外的通用数据，您可以在“编辑供应商”菜单的“通用配置面板”里，点击“从当前供应商提取”，把所有的通用数据提取到通用配置中，之后在新建“供应商”的时候，只要勾选“写入通用配置”（默认勾选），就会把插件等数据写入到新的供应商配置中。您的所有配置项都会保存在运行本软件的时候，第一次导入的默认供应商里面，不会丢失。

</details>

<details>
<summary><strong>macOS 安装</strong></summary>

YKW Bridge macOS 版本已通过 Apple 代码签名和公证，可直接下载安装，无需额外操作。推荐使用 `.dmg` 安装包。

</details>

<details>
<summary><strong>为什么总有一个正在激活中的供应商无法删除？</strong></summary>

本软件的设计原则是“最小侵入性”，即使卸载本软件，也不会影响 Claude Code 的正常使用。

所以系统总会保留一个正在激活中的配置，因为如果将所有配置全部删除，Claude 将没有可用的供应商配置。如果你想切换回官方登录，可以参考下条。

</details>

<details>
<summary><strong>如何切换回官方登录？</strong></summary>

可以在预设供应商里面添加一个官方 Claude 供应商。切换过去之后，执行一遍 Log out / Log in 流程，之后便可以在官方供应商和第三方供应商之间随意切换。

</details>

<details>
<summary><strong>我的数据存储在哪里？</strong></summary>

- **数据库**：`~/.ykw-bridge/ykw-bridge.db`（SQLite — 供应商、MCP、提示词、技能）
- **本地设置**：`~/.ykw-bridge/settings.json`（设备级 UI 偏好设置）
- **备份**：`~/.ykw-bridge/backups/`（自动轮换，保留最近 10 个）
- **SKILLS**：`~/.ykw-bridge/skills/`（默认同步到 Claude 工作流）
- **技能备份**：`~/.ykw-bridge/skill-backups/`（卸载前自动创建，保留最近 20 个）

</details>

## 文档

如需了解各项功能的详细使用方法，请查阅 **[用户手册](docs/user-manual/zh/README.md)** — 涵盖供应商管理、MCP/Prompts/Skills、代理与故障转移等全部功能。

## 快速开始

### 基本使用

1. **添加供应商**：点击"添加供应商" → 选择预设或创建自定义配置
2. **切换供应商**：
   - 主界面：选择供应商 → 点击"启用"
   - 系统托盘：直接点击供应商名称（立即生效）
3. **生效方式**：重启终端或对应的 CLI 工具以应用更改（Claude Code 无需重启）
4. **恢复官方登录**：添加"官方登录"预设，重启 CLI 工具后按照其登录/OAuth 流程操作

### MCP、Prompts、Skills 与会话

- **MCP**：点击"MCP"按钮 → 通过模板或自定义配置添加服务器 → 同步到 Claude 路径
- **Prompts**：点击"Prompts" → 使用 Markdown 编辑器创建预设 → 激活后同步到 live 文件
- **Skills**：点击"Skills" → 浏览 GitHub 仓库 → 一键安装到 Claude 工作流
- **会话**：点击"Sessions" → 浏览、搜索和恢复 Claude 对话历史

> **注意**：首次启动可以手动导入现有 Claude 配置作为默认供应商。

## 下载安装

### 系统要求

- **Windows**：Windows 10 及以上
- **macOS**：macOS 12 (Monterey) 及以上
- **Linux**：Ubuntu 22.04+ / Debian 11+ / Fedora 34+ 等主流发行版

### Windows 用户

从 [Releases](../../releases) 页面下载最新的 Windows 安装包（`.msi`）或便携压缩包（`.zip`）。

### macOS 用户

**手动下载（当前推荐）**

从 [Releases](../../releases) 页面下载最新的 macOS `.dmg`（推荐）或 `.zip`。

> **注意**：YKW Bridge macOS 版本已通过 Apple 代码签名和公证，可直接安装打开。

### Arch Linux 用户

当前建议直接使用 [Releases](../../releases) 页面提供的最新版 `.AppImage`。

### Linux 用户

从 [Releases](../../releases) 页面下载最新版本的 Linux 安装包：

- `.deb`（Debian/Ubuntu）
- `.rpm`（Fedora/RHEL/openSUSE）
- `.AppImage`（通用）

> **Flatpak**：官方 Release 不包含 Flatpak 包。如需使用，可从 `.deb` 自行构建 — 参见 [`flatpak/README.md`](flatpak/README.md)。

<details>
<summary><strong>架构总览</strong></summary>

### 设计原则

```
┌─────────────────────────────────────────────────────────────┐
│                    前端 (React + TS)                         │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │ Components  │  │    Hooks     │  │  TanStack Query  │    │
│  │   （UI）     │──│ （业务逻辑）   │──│   （缓存/同步）    │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└────────────────────────┬────────────────────────────────────┘
                         │ Tauri IPC
┌────────────────────────▼────────────────────────────────────┐
│                  后端 (Tauri + Rust)                         │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │  Commands   │  │   Services   │  │  Models/Config   │    │
│  │ （API 层）   │──│  （业务层）    │──│    （数据）       │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**核心设计模式**

- **SSOT**（单一事实源）：所有数据存储在 `~/.ykw-bridge/ykw-bridge.db`（SQLite）
- **双层存储**：SQLite 存储可同步数据，JSON 存储设备级设置
- **双向同步**：切换时写入 live 文件，编辑当前供应商时从 live 回填
- **原子写入**：临时文件 + 重命名模式防止配置损坏
- **并发安全**：Mutex 保护的数据库连接避免竞态条件
- **分层架构**：清晰分离（Commands → Services → DAO → Database）

**核心组件**

- **ProviderService**：供应商增删改查、切换、回填、排序
- **McpService**：MCP 服务器管理、导入导出、live 文件同步
- **ProxyService**：本地 Proxy 模式，支持热切换和格式转换
- **SessionManager**：全应用会话历史浏览
- **ConfigService**：配置导入导出、备份轮换
- **SpeedtestService**：API 端点延迟测量

</details>

<details>
<summary><strong>开发指南</strong></summary>

### 环境要求

- Node.js 18+
- pnpm 8+
- Rust 1.85+
- Tauri CLI 2.8+

### 开发命令

```bash
# 安装依赖
pnpm install

# 开发模式（热重载）
pnpm dev

# 类型检查
pnpm typecheck

# 代码格式化
pnpm format

# 检查代码格式
pnpm format:check

# 运行前端单元测试
pnpm test:unit

# 监听模式运行测试（推荐开发时使用）
pnpm test:unit:watch

# 构建应用
pnpm build

# 构建调试版本
pnpm tauri build --debug
```

### Rust 后端开发

```bash
cd src-tauri

# 格式化 Rust 代码
cargo fmt

# 运行 clippy 检查
cargo clippy

# 运行后端测试
cargo test

# 运行特定测试
cargo test test_name

# 运行带测试 hooks 的测试
cargo test --features test-hooks
```

### 测试说明

**前端测试**：

- 使用 **vitest** 作为测试框架
- 使用 **MSW (Mock Service Worker)** 模拟 Tauri API 调用
- 使用 **@testing-library/react** 进行组件测试

**运行测试**：

```bash
# 运行所有测试
pnpm test:unit

# 监听模式（自动重跑）
pnpm test:unit:watch

# 带覆盖率报告
pnpm test:unit --coverage
```

### 技术栈

**前端**：React 18 · TypeScript · Vite · TailwindCSS 3.4 · TanStack Query v5 · react-i18next · react-hook-form · zod · shadcn/ui · @dnd-kit

**后端**：Tauri 2.8 · Rust · serde · tokio · thiserror · tauri-plugin-updater/process/dialog/store/log

**测试**：vitest · MSW · @testing-library/react

</details>

<details>
<summary><strong>项目结构</strong></summary>

```
├── src/                        # 前端 (React + TypeScript)
│   ├── components/
│   │   ├── providers/          # 供应商管理
│   │   ├── mcp/                # MCP 面板
│   │   ├── prompts/            # Prompts 管理
│   │   ├── skills/             # Skills 管理
│   │   ├── sessions/           # 会话管理器
│   │   ├── proxy/              # Proxy 模式面板
│   │   ├── settings/           # 设置（终端/备份/关于）
│   │   ├── deeplink/           # Deep Link 导入
│   │   ├── env/                # 环境变量管理
│   │   ├── usage/              # 用量统计
│   │   └── ui/                 # shadcn/ui 组件库
│   ├── hooks/                  # 自定义 hooks（业务逻辑）
│   ├── lib/
│   │   ├── api/                # Tauri API 封装（类型安全）
│   │   └── query/              # TanStack Query 配置
│   ├── locales/                # 翻译 (zh/en/ja)
│   ├── config/                 # 预设 (providers/mcp)
│   └── types/                  # TypeScript 类型定义
├── src-tauri/                  # 后端 (Rust)
│   └── src/
│       ├── commands/           # Tauri 命令层（按领域）
│       ├── services/           # 业务逻辑层
│       ├── database/           # SQLite DAO 层
│       ├── proxy/              # Proxy 模块
│       ├── session_manager/    # 会话管理
│       ├── deeplink/           # Deep Link 处理
│       └── mcp/                # MCP 同步模块
├── tests/                      # 前端测试
└── assets/                     # 截图 & 合作商资源
```

</details>

## 贡献

欢迎提交 Issue 反馈问题和建议！

提交 PR 前请确保：

- 通过类型检查：`pnpm typecheck`
- 通过格式检查：`pnpm format:check`
- 通过单元测试：`pnpm test:unit`

新功能开发前，欢迎先开 Issue 讨论实现方案，不适合项目的功能性 PR 有可能会被关闭。

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=SherlockShemol/ykw-bridge&type=Date)](https://www.star-history.com/#SherlockShemol/ykw-bridge&Date)

## License

本项目基于 MIT License 发布。详见 [LICENSE](LICENSE)。原项目的版权声明已按照许可证要求保留。
