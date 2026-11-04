# westerm

[![CI](https://github.com/SteelCrab/westerm/actions/workflows/ci.yml/badge.svg)](https://github.com/SteelCrab/westerm/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/SteelCrab/westerm?sort=semver)](https://github.com/SteelCrab/westerm/releases/latest)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue)](https://github.com/SteelCrab/westerm/releases/latest)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange?logo=rust)](native)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

**We**b **S**ession **Term**inal. 창을 닫아도, 프로그램을 다시 켜도, 재부팅해도 세션이 남는 웹 터미널입니다.

[English](README.md) · **한국어** · [日本語](README.ja.md) · [中文](README.zh-CN.md)

![westerm: terminal-browser, 셸, terminal-code를 한 탭에 분할 배치한 화면](docs/screenshot.png)

셸, terminal-browser, terminal-code, SSH를 탭과 분할 패널로 배치합니다.
화면은 [wterm](https://github.com/vercel-labs/wterm)과 Ghostty WASM 코어로 그립니다. westerm은 wterm 공식 프로젝트가 아닙니다.

사용법, 세션, 단축키, 구조는 [문서](docs/README.md)(영어)에 있습니다.

## 실행

### 릴리스 설치

macOS, Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/SteelCrab/westerm/main/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/SteelCrab/westerm/main/install.ps1 | iex
```

설치 후 `westerm`을 실행하고 http://127.0.0.1:4317 을 엽니다.

- 스크립트가 OS와 CPU에 맞는 빌드를 고르고, SHA-256을 확인하고, 설치합니다.
  - macOS, Linux: `~/.local/bin`
  - Windows: `%LOCALAPPDATA%\Programs\westerm` (사용자 PATH에 추가)
- 버전은 `WESTERM_VERSION=v0.1.0`, 폴더는 `WESTERM_INSTALL_DIR`로 정합니다.
- 업데이트는 대시보드의 업데이트 버튼, `westerm update`, 또는 스크립트 재실행으로 합니다. 열려 있던 셸은 한 번 "복원됨"으로 다시 시작합니다.
- 직접 설치하려면 [Releases](https://github.com/SteelCrab/westerm/releases/latest)에서 압축 파일을 받습니다.

| 시스템 | 파일 |
|---|---|
| macOS, Apple Silicon | `westerm-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `westerm-x86_64-apple-darwin.tar.gz` |
| Linux x64 (정적 빌드) | `westerm-x86_64-unknown-linux-musl.tar.gz` |
| Linux arm64 (정적 빌드) | `westerm-aarch64-unknown-linux-musl.tar.gz` |
| Windows x64, arm64 | `westerm-x86_64-pc-windows-msvc.zip` |

  macOS는 브라우저로 받은 파일을 처음 한 번 막습니다. `xattr -d com.apple.quarantine westerm`으로 허용합니다.

### macOS 앱

[Releases](https://github.com/SteelCrab/westerm/releases/latest)에서 `westerm-macos-app.zip`(Apple Silicon, Intel 공용)을 받아 압축을 풀고, **westerm.app**을 응용 프로그램 폴더로 옮깁니다. 같은 대시보드를 앱 창으로 엽니다.

- 4317 포트에 westerm이 이미 실행 중이면(예: 터미널에서 실행) 앱이 그 westerm을 엽니다. 그래서 두 곳에 같은 세션이 보입니다. 없으면 앱이 직접 실행합니다.
- 앱을 종료해도 셸은 CLI처럼 계속 실행됩니다. 저장하고 끄려면 `/Applications/westerm.app/Contents/MacOS/westerm stop`을 실행합니다.
- 앱은 Apple 공증을 받지 않아서 macOS가 처음 한 번 실행을 막습니다. `xattr -dr com.apple.quarantine /Applications/westerm.app`을 실행하거나, 시스템 설정 ▸ 개인정보 보호 및 보안에서 "그래도 열기"를 누릅니다.
- 직접 빌드: `npm run build:app` (`dist-app/westerm.app` 생성)

### 소스에서 빌드

빌드에는 Node.js 24 이상과 Rust(cargo)가 필요합니다.

```bash
npm install
npm run build
npm start          # http://127.0.0.1:4317
```

- 빌드 결과는 실행 파일 하나입니다: `native/target/release/westerm`(Windows는 `westerm.exe`). 웹 화면이 들어 있어서 **실행에는 Node가 필요 없습니다.**
- 포트는 `westerm --port N` 또는 `PORT` 환경 변수로 바꿉니다.
- `westerm stop`은 백그라운드 PTY 호스트가 모든 세션을 저장하고 끝나게 합니다.

### 플랫폼

| | macOS, Linux | Windows |
|---|---|---|
| 셸 | 로그인 셸(`$SHELL`) | PowerShell 7(`pwsh`), 없으면 Windows PowerShell |
| SSH | `ssh`와 `~/.ssh/config` | 내장 OpenSSH 클라이언트와 `%USERPROFILE%\.ssh\config` |
| terminal-browser, terminal-code | 설치되어 있으면 사용(아래 표) | 아직 안 됨: Windows ConPTY가 Kitty 그래픽을 전달하지 않습니다 |

아이콘별 프로그램이 PATH에 있어야 합니다. macOS와 Linux는 로그인 셸의 PATH입니다.

| 아이콘 | 설치 |
|---|---|
| terminal-browser | `brew install --cask terminal-browser` |
| terminal-code | `curl -fsSL https://tode.sh/install \| bash` (명령 이름 `tode`) |

## 개발

```bash
npm run dev:server   # Rust 서버 (:4317), Vite 주소를 허용 출처로 추가
npm run dev          # Vite HMR (:5173), /api와 /ws를 :4317로 전달

npm test             # 화면 테스트(node:test) + Rust 테스트(cargo test)
npm run type-check
```

| 경로 | 내용 |
|---|---|
| `src/` | 화면 (React + wterm) |
| `src/i18n/` | 화면 문구 (한국어, English, 日本語, 中文) |
| `shared/` | 프로토콜 타입, 레이아웃 트리, 저장 형식 |
| `native/src/server/` | 대시보드 서버 (axum) |
| `native/src/host/` | PTY 호스트: 셸 유지, 화면 저장·복원 |
| `native/src/ipc.rs` | 대시보드 ↔ 호스트 통로: Unix 소켓, Windows는 named pipe |
| `macos/` | macOS 앱: Swift 창(WKWebView), 아이콘, 빌드 스크립트 |

CI가 모든 PR을 macOS, Linux, Windows에서 시험합니다. PTY 호스트 코드를 바꾸면 실행 중인 셸이 한 번 `복원됨`으로 다시 시작합니다. 자세한 내용은 [Sessions](docs/sessions.md)(영어)를 보세요.

## 기여

- 버그와 제안은 [Issues](https://github.com/SteelCrab/westerm/issues)에 올려 주세요.
- PR 전에 `npm test`와 `npm run type-check`를 통과시켜 주세요.
- 개발 준비, 규칙, 번역, 릴리스 방법은 [기여 안내](CONTRIBUTING.ko.md)를 보세요.
- 보안 문제는 공개 이슈 대신 [비공개 취약점 제보](https://github.com/SteelCrab/westerm/security/advisories/new)로 알려 주세요.

westerm은 [MIT 라이선스](LICENSE)로 배포합니다. 기여도 같은 라이선스로 받습니다.
