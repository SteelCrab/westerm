# westerm

**We**b **S**ession **Term**inal。关闭窗口、重启程序、重启电脑之后，会话依然保留的 Web 终端。

[English](README.md) · [한국어](README.ko.md) · [日本語](README.ja.md) · **中文**

![westerm：在一个标签页中分屏排列 terminal-browser、Shell 和 terminal-code](docs/screenshot.png)

用标签页和分屏面板排列 Shell、terminal-browser、terminal-code 和 SSH 会话。
界面由 [wterm](https://github.com/vercel-labs/wterm) 和 Ghostty WASM 核心绘制。westerm 不是 wterm 的官方项目。

使用方法和架构说明见[文档](docs/README.md)（韩语）。

## 运行

构建需要 Node.js 24 或更高版本以及 Rust（cargo）。westerm 在 macOS 上开发和测试。

```bash
npm install
npm run build
npm start          # http://127.0.0.1:4317
```

- 构建结果是一个可执行文件：`native/target/release/westerm`。其中内置了 Web 界面，所以**运行时不需要 Node。**
- 用 `westerm --port N` 或环境变量 `PORT` 更改端口。
- 每个启动器对应的程序必须在登录 Shell 的 PATH 中。

| 启动器 | 安装 |
|---|---|
| terminal-browser | `brew install --cask terminal-browser` |
| terminal-code | `curl -fsSL https://tode.sh/install \| bash`（命令名为 `tode`） |
| SSH | 系统自带的 `ssh` 和 `~/.ssh/config` |

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

修改 PTY 主机代码后，正在运行的 Shell 会以“已恢复”状态重新启动一次。详见[会话保持](docs/sessions.md)（韩语）。

## 贡献

- 请在 [Issues](https://github.com/SteelCrab/westerm/issues) 中报告问题和建议。
- 提交 PR 前，请确保 `npm test` 和 `npm run type-check` 通过。
- 开发环境、规则和翻译方法见 [CONTRIBUTING.md](CONTRIBUTING.md)（英语）。
- 安全问题请通过[私密漏洞报告](https://github.com/SteelCrab/westerm/security/advisories/new)告知，不要发公开 Issue。
