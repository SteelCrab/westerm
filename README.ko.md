# westerm

**We**b **S**ession **Term**inal. 창을 닫아도, 프로그램을 다시 켜도, 재부팅해도 세션이 남는 웹 터미널입니다.

[English](README.md) · **한국어** · [日本語](README.ja.md) · [中文](README.zh-CN.md)

![westerm 데모](assets/demo.gif)

![westerm: terminal-browser, 셸, terminal-code를 한 탭에 분할 배치한 화면](assets/screenshot.png)

## 설치

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

```bash
westerm
```

그다음 http://127.0.0.1:4317 을 엽니다.

- `westerm stop`: 모든 세션을 저장하고 멈춥니다.
- `westerm update`: 최신 버전을 설치합니다.
- `westerm --port 4318`: 다른 포트를 씁니다.

화면 사용법: [docs/usage.md](docs/usage.md)

## 데스크톱 앱

데스크톱 앱은 같은 화면을 자체 창으로 보여 줍니다. macOS, Windows, Linux에서 씁니다.

### 설치

[Releases](https://github.com/SteelCrab/westerm/releases/latest)에서 받습니다.

- macOS: `westerm-macos-app.dmg`를 열고 **westerm**을 응용 프로그램 폴더로 드래그합니다.
- Windows: `westerm-windows-app-setup.exe`를 실행합니다.
- Linux: `westerm-linux-app-<arch>.deb` 또는 `.rpm`을 설치합니다.

macOS에서 앱의 첫 실행을 허용하는 방법: [docs/install.md](docs/install.md#desktop-app)

### 실행

- macOS: 응용 프로그램 폴더에서 **westerm**을 엽니다.
- Windows: 시작 메뉴에서 **westerm**을 엽니다.
- Linux: 앱 메뉴에서 **westerm**을 열거나, 터미널에서 `westerm-app`을 실행합니다.

## 소스에서 빌드

빌드하려면 Node.js 24 이상과 Rust가 필요합니다.

```bash
git clone https://github.com/SteelCrab/westerm.git
cd westerm
npm install
npm run build
npm start
```

- `npm start`는 빌드된 `native/target/release/westerm`을 실행합니다. 그다음 http://127.0.0.1:4317 을 엽니다.
- `npm run build:app`은 데스크톱 앱을 `dist-app/`에 빌드합니다.

자세한 내용: [docs/install.md](docs/install.md#build-from-source)

## 기여

버그와 아이디어는 [Issues](https://github.com/SteelCrab/westerm/issues)에 올립니다. PR 전에 [CONTRIBUTING.ko.md](CONTRIBUTING.ko.md)를 읽어 주세요.

westerm은 [MIT 라이선스](LICENSE)입니다.
