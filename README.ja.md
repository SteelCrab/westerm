# westerm

**We**b **S**ession **Term**inal。ウィンドウを閉じても、プログラムを再起動しても、再起動後もセッションが残る Web ターミナルです。

[English](README.md) · [한국어](README.ko.md) · **日本語** · [中文](README.zh-CN.md)

![westerm のデモ](assets/demo.gif)

![westerm: terminal-browser、シェル、terminal-code を 1 つのタブに分割配置した画面](assets/screenshot.png)

## インストール

### デスクトップアプリ(おすすめ)

いちばん簡単なインストール方法です。お使いのシステム用のファイルをダウンロードして開きます。ターミナル、Node.js、Rust は不要です。

| システム | ダウンロード | インストール |
|---|---|---|
| macOS | [`westerm-macos-app.dmg`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-macos-app.dmg) | ファイルを開き、**westerm** をアプリケーションフォルダへドラッグします。 |
| Windows | [`westerm-windows-app-setup.exe`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-windows-app-setup.exe) | ファイルを実行します。 |
| Linux x64 | [`.deb`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-linux-app-amd64.deb) · [`.rpm`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-linux-app-x86_64.rpm) | パッケージをインストールします。 |
| Linux arm64 | [`.deb`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-linux-app-arm64.deb) · [`.rpm`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-linux-app-aarch64.rpm) | パッケージをインストールします。 |

次に **westerm** を開きます。ダッシュボードが専用ウィンドウで開きます。

- macOS は初回の起動をブロックします。システム設定 ▸ プライバシーとセキュリティ ▸「このまま開く」で一度だけ許可します。
- Windows SmartScreen が一度だけ確認することがあります。詳細情報 ▸ 実行 を選びます。

アプリの詳細: [docs/install.md](docs/install.md#desktop-app)(英語)

### コマンドライン

`westerm` コマンドをインストールします。アプリのウィンドウはありません。サーバーで使うときや、ターミナルのほうが使いやすいときに使います。

#### npm

Node.js 22 以降が必要です。

```bash
npm install -g @pistacrab/westerm
```

`npx @pistacrab/westerm` で直接実行することもできます。npm でインストールした場合は `npm install -g @pistacrab/westerm@latest` で更新します。

#### インストールスクリプト

macOS、Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/SteelCrab/westerm/main/install.sh | sh
```

Windows(PowerShell):

```powershell
irm https://raw.githubusercontent.com/SteelCrab/westerm/main/install.ps1 | iex
```

ほかのインストール方法: [docs/install.md](docs/install.md)(英語)

## 実行

デスクトップアプリ: **westerm** を開きます。

コマンドライン:

```bash
westerm
```

次に http://127.0.0.1:4317 を開きます。

- `westerm stop`: すべてのセッションを保存して停止します。
- `westerm update`: 最新リリースをインストールします。
- `westerm --port 4318`: 別のポートを使います。

画面の使い方: [docs/usage.md](docs/usage.md)(英語)

## コントリビュート

バグや提案は [Issues](https://github.com/SteelCrab/westerm/issues) へ。PR の前に [CONTRIBUTING.md](CONTRIBUTING.md)(英語)を読んでください。

westerm は [MIT ライセンス](LICENSE)です。
