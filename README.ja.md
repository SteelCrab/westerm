# westerm

[![CI](https://github.com/SteelCrab/westerm/actions/workflows/ci.yml/badge.svg)](https://github.com/SteelCrab/westerm/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/SteelCrab/westerm?sort=semver)](https://github.com/SteelCrab/westerm/releases/latest)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue)](https://github.com/SteelCrab/westerm/releases/latest)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)](native)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

**We**b **S**ession **Term**inal。ウィンドウを閉じても、プログラムを再起動しても、再起動後もセッションが残る Web ターミナルです。

[English](README.md) · [한국어](README.ko.md) · **日本語** · [中文](README.zh-CN.md)

![westerm: terminal-browser、シェル、terminal-code を 1 つのタブに分割配置した画面](docs/screenshot.png)

シェル、terminal-browser、terminal-code、SSH をタブと分割パネルに配置します。
画面は [wterm](https://github.com/vercel-labs/wterm) と Ghostty WASM コアで描画します。westerm は wterm の公式プロジェクトではありません。

使い方と構成は[ドキュメント](docs/README.md)(韓国語)にあります。

## 実行

### リリースをインストール

macOS、Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/SteelCrab/westerm/main/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/SteelCrab/westerm/main/install.ps1 | iex
```

インストール後、`westerm` を実行して http://127.0.0.1:4317 を開きます。

- スクリプトは OS と CPU に合うビルドを選び、SHA-256 を確認してインストールします。
  - macOS、Linux: `~/.local/bin`
  - Windows: `%LOCALAPPDATA%\Programs\westerm`(ユーザーの PATH に追加)
- バージョンは `WESTERM_VERSION=v0.1.0`、フォルダは `WESTERM_INSTALL_DIR` で指定します。
- 更新するにはスクリプトを再実行し、westerm を再起動します。開いていたシェルが一度「復元済み」として再起動することがあります。
- 手動でインストールする場合は [Releases](https://github.com/SteelCrab/westerm/releases/latest) からアーカイブを取得します。

| システム | ファイル |
|---|---|
| macOS、Apple シリコン | `westerm-aarch64-apple-darwin.tar.gz` |
| macOS、Intel | `westerm-x86_64-apple-darwin.tar.gz` |
| Linux x64(静的ビルド) | `westerm-x86_64-unknown-linux-musl.tar.gz` |
| Linux arm64(静的ビルド) | `westerm-aarch64-unknown-linux-musl.tar.gz` |
| Windows x64、arm64 | `westerm-x86_64-pc-windows-msvc.zip` |

  macOS はブラウザで取得したファイルを最初に一度ブロックします。`xattr -d com.apple.quarantine westerm` で許可します。

### macOS アプリ

[Releases](https://github.com/SteelCrab/westerm/releases/latest) から `westerm-macos-app.zip`(Apple シリコン、Intel 共通)を取得して展開し、**westerm.app** をアプリケーションフォルダへ移動します。同じダッシュボードを専用ウィンドウで開きます。

- ポート 4317 で westerm がすでに動いていれば(ターミナルから起動したものなど)、アプリはそれを開くので、両方で同じセッションが見えます。なければアプリが自分で起動します。
- アプリを終了しても、シェルは CLI と同じく動き続けます。保存して止めるには `/Applications/westerm.app/Contents/MacOS/westerm stop` を実行します。
- アプリは Apple の公証を受けていないため、macOS が最初の起動を一度ブロックします。`xattr -dr com.apple.quarantine /Applications/westerm.app` を実行するか、システム設定 ▸ プライバシーとセキュリティで「このまま開く」を選びます。
- 自分でビルドする場合: `npm run build:app`(`dist-app/westerm.app` を作成)

### ソースからビルド

ビルドには Node.js 24 以上と Rust(cargo)が必要です。

```bash
npm install
npm run build
npm start          # http://127.0.0.1:4317
```

- ビルド結果は実行ファイル 1 つです: `native/target/release/westerm`(Windows は `westerm.exe`)。Web 画面を内蔵しているので、**実行に Node は不要です。**
- ポートは `westerm --port N` または環境変数 `PORT` で変更します。
- `westerm stop` はバックグラウンドの PTY ホストにすべてのセッションを保存させて終了させます。

### プラットフォーム

| | macOS、Linux | Windows |
|---|---|---|
| シェル | ログインシェル(`$SHELL`) | PowerShell 7(`pwsh`)、なければ Windows PowerShell |
| SSH | `ssh` と `~/.ssh/config` | 内蔵の OpenSSH クライアントと `%USERPROFILE%\.ssh\config` |
| terminal-browser、terminal-code | インストールされていれば使用(下表) | 未対応: Windows の ConPTY が Kitty グラフィックスを通しません |

各ランチャーのプログラムが PATH にある必要があります。macOS と Linux ではログインシェルの PATH です。

| ランチャー | インストール |
|---|---|
| terminal-browser | `brew install --cask terminal-browser` |
| terminal-code | `curl -fsSL https://tode.sh/install \| bash`(コマンド名は `tode`) |

## 開発

```bash
npm run dev:server   # Rust サーバー (:4317)、Vite のオリジンを許可
npm run dev          # Vite HMR (:5173)、/api と /ws を :4317 へ転送

npm test             # 画面のテスト(node:test)+ Rust のテスト(cargo test)
npm run type-check
```

| パス | 内容 |
|---|---|
| `src/` | 画面 (React + wterm) |
| `src/i18n/` | 画面の文言 (한국어, English, 日本語, 中文) |
| `shared/` | プロトコルの型、レイアウトツリー、保存形式 |
| `native/src/server/` | ダッシュボードサーバー (axum) |
| `native/src/host/` | PTY ホスト: シェルの維持、画面の保存と復元 |
| `native/src/ipc.rs` | ダッシュボード ↔ ホスト間の通路: Unix ソケット、Windows は名前付きパイプ |
| `macos/` | macOS アプリ: Swift のウィンドウ(WKWebView)、アイコン、ビルドスクリプト |

CI はすべての PR を macOS、Linux、Windows でテストします。PTY ホストのコードを変更すると、実行中のシェルが一度「復元済み」として再起動します。詳しくは[セッションの維持](docs/sessions.md)(韓国語)を参照してください。

## コントリビュート

- バグや提案は [Issues](https://github.com/SteelCrab/westerm/issues) へ。
- PR の前に `npm test` と `npm run type-check` を通してください。
- 開発環境、ルール、翻訳、リリースの方法は [CONTRIBUTING.md](CONTRIBUTING.md)(英語)を参照してください。
- セキュリティの問題は公開 Issue ではなく[非公開の脆弱性報告](https://github.com/SteelCrab/westerm/security/advisories/new)で知らせてください。

westerm は [MIT ライセンス](LICENSE)で公開しています。コントリビュートも同じライセンスで受け付けます。
