# westerm

**We**b **S**ession **Term**inal. 창을 닫아도, 프로그램을 다시 켜도, 재부팅해도 세션이 남는 웹 터미널입니다.

[English](README.md) · **한국어** · [日本語](README.ja.md) · [中文](README.zh-CN.md)

![westerm 데모](assets/demo.gif)

![westerm: terminal-browser, 셸, terminal-code를 한 탭에 분할 배치한 화면](assets/screenshot.png)

## 설치

### 데스크톱 앱 (추천)

가장 쉬운 설치 방법입니다. 내 시스템에 맞는 파일을 받아서 엽니다. 터미널, Node.js, Rust가 필요 없습니다.

| 시스템 | 다운로드 | 설치 |
|---|---|---|
| macOS | [`westerm-macos-app.dmg`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-macos-app.dmg) | 파일을 열고 **westerm**을 응용 프로그램 폴더로 드래그합니다. |
| Windows | [`westerm-windows-app-setup.exe`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-windows-app-setup.exe) | 파일을 실행합니다. |
| Linux x64 | [`.deb`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-linux-app-amd64.deb) · [`.rpm`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-linux-app-x86_64.rpm) | 패키지를 설치합니다. |
| Linux arm64 | [`.deb`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-linux-app-arm64.deb) · [`.rpm`](https://github.com/SteelCrab/westerm/releases/latest/download/westerm-linux-app-aarch64.rpm) | 패키지를 설치합니다. |

그다음 **westerm**을 엽니다. 대시보드가 앱 창에서 열립니다.

- macOS는 첫 실행을 막습니다. 시스템 설정 ▸ 개인정보 보호 및 보안 ▸ 그래도 열기에서 한 번 허용합니다.
- Windows SmartScreen이 한 번 물을 수 있습니다. 추가 정보 ▸ 실행을 고릅니다.

앱에 대한 자세한 내용: [docs/install.md](docs/install.md#desktop-app)

### 명령줄

`westerm` 명령을 설치합니다. 앱 창은 없습니다. 서버에서 쓰거나 터미널이 편할 때 씁니다.

#### npm

Node.js 22 이상이 필요합니다.

```bash
npm install -g @pistacrab/westerm
```

`npx @pistacrab/westerm`으로 바로 실행할 수도 있습니다. npm 설치본은 `npm install -g @pistacrab/westerm@latest`로 업데이트합니다.

#### Bun

```bash
bun add -g @pistacrab/westerm
```

같은 npm 패키지를 설치합니다. 설치된 `westerm` 명령은 Node.js 22 이상으로 시작합니다. Node.js가 없으면 `bunx @pistacrab/westerm`으로 실행합니다. `bun add -g @pistacrab/westerm@latest`로 업데이트합니다.

#### 설치 스크립트

macOS, Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/SteelCrab/westerm/main/install.sh | sh
```

Windows(PowerShell):

```powershell
irm https://raw.githubusercontent.com/SteelCrab/westerm/main/install.ps1 | iex
```

다른 설치 방법: [docs/install.md](docs/install.md)

## 실행

데스크톱 앱: **westerm**을 엽니다.

명령줄:

```bash
westerm
```

그다음 http://127.0.0.1:4317 을 엽니다.

- `westerm stop`: 모든 세션을 저장하고 멈춥니다.
- `westerm update`: 최신 버전을 설치합니다.
- `westerm --port 4318`: 다른 포트를 씁니다.

화면 사용법: [docs/usage.md](docs/usage.md)

## 기여

버그와 아이디어는 [Issues](https://github.com/SteelCrab/westerm/issues)에 올립니다. PR 전에 [CONTRIBUTING.md](CONTRIBUTING.md)(영어)를 읽어 주세요.

westerm은 [MIT 라이선스](LICENSE)입니다.
