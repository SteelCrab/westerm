# westerm

[![CI](https://github.com/SteelCrab/westerm/actions/workflows/ci.yml/badge.svg)](https://github.com/SteelCrab/westerm/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/SteelCrab/westerm?sort=semver)](https://github.com/SteelCrab/westerm/releases/latest)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue)](https://github.com/SteelCrab/westerm/releases/latest)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)](native)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

**We**b **S**ession **Term**inal。关闭窗口、重启程序、重启电脑之后，会话依然保留的 Web 终端。

[English](README.md) · [한국어](README.ko.md) · [日本語](README.ja.md) · **中文**

![westerm：在一个标签页中分屏排列 terminal-browser、Shell 和 terminal-code](docs/screenshot.png)

用标签页和分屏面板排列 Shell、terminal-browser、terminal-code 和 SSH 会话。
界面由 [wterm](https://github.com/vercel-labs/wterm) 和 Ghostty WASM 核心绘制。westerm 不是 wterm 的官方项目。

使用方法、会话、快捷键和架构说明见[文档](docs/README.md)（英语）。

## 运行

### 安装发布版本

macOS、Linux：

```bash
curl -fsSL https://raw.githubusercontent.com/SteelCrab/westerm/main/install.sh | sh
```

Windows（PowerShell）：

```powershell
irm https://raw.githubusercontent.com/SteelCrab/westerm/main/install.ps1 | iex
```

安装后运行 `westerm`，然后打开 http://127.0.0.1:4317。

- 脚本会选择适合你的操作系统和 CPU 的构建，校验 SHA-256，然后安装：
  - macOS、Linux：`~/.local/bin`
  - Windows：`%LOCALAPPDATA%\Programs\westerm`（并加入用户 PATH）
- 用 `WESTERM_VERSION=v0.1.0` 指定版本，用 `WESTERM_INSTALL_DIR` 指定目录。
- 更新可以用仪表板中的更新按钮、`westerm update`，或再次运行脚本。已打开的 Shell 会以“已恢复”状态重新启动一次。
- 手动安装时，从 [Releases](https://github.com/SteelCrab/westerm/releases/latest) 下载压缩包：

| 系统 | 文件 |
|---|---|
| macOS，Apple 芯片 | `westerm-aarch64-apple-darwin.tar.gz` |
| macOS，Intel | `westerm-x86_64-apple-darwin.tar.gz` |
| Linux x64（静态构建） | `westerm-x86_64-unknown-linux-musl.tar.gz` |
| Linux arm64（静态构建） | `westerm-aarch64-unknown-linux-musl.tar.gz` |
| Windows x64、arm64 | `westerm-x86_64-pc-windows-msvc.zip` |

  macOS 会拦截用浏览器下载的文件一次。用 `xattr -d com.apple.quarantine westerm` 允许运行。

### macOS 应用

从 [Releases](https://github.com/SteelCrab/westerm/releases/latest) 下载 `westerm-macos-app.dmg`（Apple 芯片和 Intel 通用）并打开。在弹出的窗口中把 **westerm** 拖到“应用程序”文件夹，然后推出磁盘映像。应用在独立窗口中打开同一个仪表板。

- 如果端口 4317 上已经运行着 westerm（例如从终端启动的），应用会打开它，两边显示相同的会话；否则应用自己启动一个。
- 退出应用后，Shell 会像 CLI 一样继续运行。要保存并停止它们，运行 `/Applications/westerm.app/Contents/MacOS/westerm stop`。
- 应用尚未经过 Apple 公证，macOS 会拦截第一次打开。运行 `xattr -dr com.apple.quarantine /Applications/westerm.app`，或在“系统设置 ▸ 隐私与安全性”中点“仍要打开”。
- 自行构建：`npm run build:app`（生成 `dist-app/westerm.app`）

### 从源码构建

构建需要 Node.js 24 或更高版本以及 Rust（cargo）。

```bash
npm install
npm run build
npm start          # http://127.0.0.1:4317
```

- 构建结果是一个可执行文件：`native/target/release/westerm`（Windows 为 `westerm.exe`）。其中内置了 Web 界面，所以**运行时不需要 Node。**
- 用 `westerm --port N` 或环境变量 `PORT` 更改端口。
- `westerm stop` 让后台的 PTY 主机保存所有会话后退出。

### 平台

| | macOS、Linux | Windows |
|---|---|---|
| Shell | 登录 Shell（`$SHELL`） | PowerShell 7（`pwsh`），没有则用 Windows PowerShell |
| SSH | `ssh` 和 `~/.ssh/config` | 内置 OpenSSH 客户端和 `%USERPROFILE%\.ssh\config` |
| terminal-browser、terminal-code | 已安装时可用（见下表） | 暂不支持：Windows ConPTY 不传递 Kitty 图形 |

每个启动器对应的程序必须在 PATH 中。macOS 和 Linux 上是登录 Shell 的 PATH。

| 启动器 | 安装 |
|---|---|
| terminal-browser | `brew install --cask terminal-browser` |
| terminal-code | `curl -fsSL https://tode.sh/install \| bash`（命令名为 `tode`） |

## 开发

```bash
npm run dev:server   # Rust 服务器 (:4317)，允许 Vite 的来源
npm run dev          # Vite HMR (:5173)，把 /api 和 /ws 转发到 :4317

npm test             # 界面测试（node:test）+ Rust 测试（cargo test）
npm run type-check
```

| 路径 | 内容 |
|---|---|
| `src/` | 界面 (React + wterm) |
| `src/i18n/` | 界面文字 (한국어, English, 日本語, 中文) |
| `shared/` | 协议类型、布局树、保存格式 |
| `native/src/server/` | 仪表板服务器 (axum) |
| `native/src/host/` | PTY 主机：保持 Shell 运行，保存和恢复屏幕 |
| `native/src/ipc.rs` | 仪表板 ↔ 主机通道：Unix 套接字，Windows 上为命名管道 |
| `macos/` | macOS 应用：Swift 窗口（WKWebView）、图标、构建脚本 |

CI 会在 macOS、Linux 和 Windows 上测试每个 PR。修改 PTY 主机代码后，正在运行的 Shell 会以“已恢复”状态重新启动一次。详见 [Sessions](docs/sessions.md)（英语）。

## 贡献

- 请在 [Issues](https://github.com/SteelCrab/westerm/issues) 中报告问题和建议。
- 提交 PR 前，请确保 `npm test` 和 `npm run type-check` 通过。
- 开发环境、规则、翻译和发布方法见 [CONTRIBUTING.md](CONTRIBUTING.md)（英语）。
- 安全问题请通过[私密漏洞报告](https://github.com/SteelCrab/westerm/security/advisories/new)告知，不要发公开 Issue。

westerm 以 [MIT 许可证](LICENSE)发布。贡献也按同一许可证接受。
