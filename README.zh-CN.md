# westerm

**We**b **S**ession **Term**inal。关闭窗口、重启程序、重启电脑之后，会话依然保留的 Web 终端。

[English](README.md) · [한국어](README.ko.md) · [日本語](README.ja.md) · **中文**

![westerm 演示](assets/demo.gif)

![westerm：在一个标签页中分屏排列 terminal-browser、Shell 和 terminal-code](assets/screenshot.png)

## 安装

### 桌面应用（推荐）

这是最简单的安装方式。下载适合你系统的文件并打开。不需要终端、Node.js 或 Rust。

| 系统 | 下载 | 安装 |
|---|---|---|
| macOS | [`westerm-macos-app.dmg`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-macos-app.dmg) | 打开文件，把 **westerm** 拖到“应用程序”文件夹。 |
| Windows | [`westerm-windows-app-setup.exe`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-windows-app-setup.exe) | 运行文件。 |
| Linux x64 | [`.deb`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-linux-app-amd64.deb) · [`.rpm`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-linux-app-x86_64.rpm) | 安装软件包。 |
| Linux arm64 | [`.deb`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-linux-app-arm64.deb) · [`.rpm`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-linux-app-aarch64.rpm) | 安装软件包。 |

然后打开 **westerm**。仪表板会在独立窗口中打开。

- macOS 会阻止首次打开。在 系统设置 ▸ 隐私与安全性 ▸ 仍要打开 中允许一次。
- Windows SmartScreen 可能会询问一次。选择 更多信息 ▸ 仍要运行。

应用的更多说明：[docs/install.md](docs/install.md#desktop-app)（英语）

### 命令行

安装 `westerm` 命令，不带应用窗口。适合在服务器上使用，或者你更习惯终端时使用。

#### npm

需要 Node.js 22 或更高版本。

```bash
npm install -g @pistacrab/westerm
```

也可以直接运行 `npx @pistacrab/westerm`。通过 npm 安装后，使用 `npm install -g @pistacrab/westerm@latest` 更新。

#### Bun

```bash
bun add -g @pistacrab/westerm
```

安装的是同一个 npm 包。安装的 `westerm` 命令通过 Node.js 22 或更高版本启动。没有 Node.js 时，运行 `bunx @pistacrab/westerm`。使用 `bun add -g @pistacrab/westerm@latest` 更新。

#### 安装脚本

macOS、Linux：

```bash
curl -fsSL https://raw.githubusercontent.com/SteelCrab/westerm/main/install.sh | sh
```

Windows（PowerShell）：

```powershell
irm https://raw.githubusercontent.com/SteelCrab/westerm/main/install.ps1 | iex
```

其他安装方式：[docs/install.md](docs/install.md)（英语）

## 运行

桌面应用：打开 **westerm**。

命令行：

```bash
westerm
```

然后打开 http://127.0.0.1:4317。

- `westerm stop`：保存所有会话并停止。
- `westerm update`：安装最新版本。
- `westerm --port 4318`：使用其他端口。

界面用法：[docs/usage.md](docs/usage.md)（英语）

## 贡献

请在 [Issues](https://github.com/SteelCrab/westerm/issues) 中报告问题和建议。提交 PR 前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)（英语）。

westerm 以 [MIT 许可证](LICENSE)发布。
