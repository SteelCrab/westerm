# westerm

**We**b **S**ession **Term**inal。关闭窗口、重启程序、重启电脑之后，会话依然保留的 Web 终端。

[English](README.md) · [한국어](README.ko.md) · [日本語](README.ja.md) · **中文**

![westerm 演示](assets/demo.gif)

![westerm：在一个标签页中分屏排列 terminal-browser、Shell 和 terminal-code](assets/screenshot.png)

## 安装

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

```bash
westerm
```

然后打开 http://127.0.0.1:4317。

- `westerm stop`：保存所有会话并停止。
- `westerm update`：安装最新版本。
- `westerm --port 4318`：使用其他端口。

界面用法：[docs/usage.md](docs/usage.md)（英语）

## 桌面应用

桌面应用在自己的窗口中显示同一界面，支持 macOS、Windows 和 Linux。

### 安装

请从 [Releases](https://github.com/SteelCrab/westerm/releases/latest) 下载：

- macOS：打开 `westerm-macos-app.dmg`，把 **westerm** 拖到“应用程序”文件夹。
- Windows：运行 `westerm-windows-app-setup.exe`。
- Linux：安装 `westerm-linux-app-<arch>.deb` 或 `.rpm`。

在 macOS 上允许应用首次打开的方法：[docs/install.md](docs/install.md#desktop-app)（英语）

### 运行

- macOS：在“应用程序”文件夹中打开 **westerm**。
- Windows：从“开始”菜单打开 **westerm**。
- Linux：从应用菜单打开 **westerm**，或在终端中运行 `westerm-app`。

## 从源码构建

构建需要 Node.js 24 或更高版本以及 Rust。

```bash
git clone https://github.com/SteelCrab/westerm.git
cd westerm
npm install
npm run build
npm start
```

- `npm start` 运行构建好的 `native/target/release/westerm`。然后打开 http://127.0.0.1:4317。
- `npm run build:app` 把桌面应用构建到 `dist-app/`。

详细说明：[docs/install.md](docs/install.md#build-from-source)（英语）

## 贡献

请在 [Issues](https://github.com/SteelCrab/westerm/issues) 中报告问题和建议。提交 PR 前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)（英语）。

westerm 以 [MIT 许可证](LICENSE)发布。
