# westerm

**We**b **S**ession **Term**inal。ウィンドウを閉じても、プログラムを再起動しても、再起動後もセッションが残る Web ターミナルです。

[English](README.md) · [한국어](README.ko.md) · **日本語** · [中文](README.zh-CN.md)

![westerm のデモ](assets/demo.gif)

![westerm: terminal-browser、シェル、terminal-code を 1 つのタブに分割配置した画面](assets/screenshot.png)

## インストール

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

```bash
westerm
```

次に http://127.0.0.1:4317 を開きます。

- `westerm stop`: すべてのセッションを保存して停止します。
- `westerm update`: 最新リリースをインストールします。
- `westerm --port 4318`: 別のポートを使います。

画面の使い方: [docs/usage.md](docs/usage.md)(英語)

## デスクトップアプリ

デスクトップアプリは同じ画面を専用のウィンドウで表示します。macOS、Windows、Linux で使えます。

### インストール

[Releases](https://github.com/SteelCrab/westerm/releases/latest) から取得します。

- macOS: `westerm-macos-app.dmg` を開き、**westerm** をアプリケーションフォルダへドラッグします。
- Windows: `westerm-windows-app-setup.exe` を実行します。
- Linux: `westerm-linux-app-<arch>.deb` または `.rpm` をインストールします。

macOS でアプリの初回起動を許可する方法: [docs/install.md](docs/install.md#desktop-app)(英語)

### 実行

- macOS: アプリケーションフォルダの **westerm** を開きます。
- Windows: スタートメニューから **westerm** を開きます。
- Linux: アプリメニューから **westerm** を開くか、ターミナルで `westerm-app` を実行します。

## ソースからビルド

ビルドには Node.js 24 以降と Rust が必要です。

```bash
git clone https://github.com/SteelCrab/westerm.git
cd westerm
npm install
npm run build
npm start
```

- `npm start` はビルドした `native/target/release/westerm` を実行します。次に http://127.0.0.1:4317 を開きます。
- `npm run build:app` はデスクトップアプリを `dist-app/` にビルドします。

詳細: [docs/install.md](docs/install.md#build-from-source)(英語)

## コントリビュート

バグや提案は [Issues](https://github.com/SteelCrab/westerm/issues) へ。PR の前に [CONTRIBUTING.md](CONTRIBUTING.md)(英語)を読んでください。

westerm は [MIT ライセンス](LICENSE)です。
