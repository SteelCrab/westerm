# westerm

**We**b **S**ession **Term**inal. 창을 닫아도, 프로그램을 다시 켜도, 재부팅해도 세션이 남는 웹 터미널입니다.
화면은 [wterm](https://github.com/vercel-labs/wterm)과 Ghostty WASM 코어(`@wterm/ghostty`)로 그립니다. westerm은 wterm 공식 프로젝트가 아닙니다.

![westerm: terminal-browser, 셸, terminal-code를 한 탭에 분할 배치한 화면](docs/screenshot.png)

셸, `terminal-browser`(브라우저), `terminal-code`(VS Code), SSH 원격 셸을 탭과 패널로 배치합니다.
세션은 새로고침, 프로그램 재시작, 컴퓨터 재시작 뒤에도 유지되거나 복원됩니다.

## 실행

서버는 Rust로 만든 **실행 파일 하나**입니다. 웹 화면도 그 안에 들어 있습니다.

```bash
npm install
npm run build      # 화면(Vite) 빌드 → Rust 실행 파일 빌드 (화면 포함)
npm start          # = ./native/target/release/westerm → http://127.0.0.1:4317
```

- 빌드에는 Node.js 24 이상과 Rust(cargo)가 필요합니다. **실행에는 Node가 필요 없습니다.**
- 실행 파일만 다른 곳에 복사해서 써도 됩니다: `westerm [--port N]`
- 포트는 `--port` 또는 `PORT` 환경 변수로 바꿉니다.
- 각 아이콘의 명령이 로그인 셸의 PATH에 있어야 합니다.
  - terminal-browser: `brew install --cask terminal-browser`
  - terminal-code: `curl -fsSL https://tode.sh/install | bash` (명령 이름은 `tode`)
  - SSH: macOS 기본 `ssh`

### 개발

```bash
npm run dev:server   # Rust 서버 (:4317), Vite 주소를 허용 출처로 추가
npm run dev          # Vite HMR (:5173), /api와 /ws를 :4317로 전달
```

### 구조

| 경로 | 내용 |
|---|---|
| `native/src/server/` | 대시보드 서버(axum): 화면 제공, API, 브라우저 WebSocket을 호스트로 중계 |
| `native/src/host/` | PTY 호스트: 프로세스 실행·유지(`portable-pty`), 화면 사본(`vt100`), 저장·복원 |
| `native/src/protocol.rs` | 메시지 형식과 검증(세션 ID, 크기, URL, SSH 대상) |
| `src/` | 화면(React + wterm Ghostty WASM) |
| `shared/` | 화면이 쓰는 프로토콜 타입, 레이아웃 트리, 저장 형식 검증 |

## 화면 구성

| 영역 | 설명 |
|---|---|
| 아이콘 바 | 새 셸, terminal-browser, terminal-code, SSH. 배지는 열린 개수입니다. |
| 사이드바 | 탭 → 세션 트리. 접기, 이름 변경, 드래그로 순서·위치 변경. 오른쪽 경계를 끌어 너비를 바꿉니다. |
| 탭 바 | `+` 또는 빈 곳 더블클릭으로 새 탭. 가운데 클릭으로 닫기. 드래그로 순서 변경. |
| 패널 | 헤더를 끌어 다른 패널의 가장자리에 놓으면 그쪽으로 분할, 가운데는 자리 바꾸기. 탭이나 `+` 위에 놓아 이동. 경계선을 끌어 크기 조절. |
| 헤더 | 명령 팔레트, 폰트 크기(A−/A+) |

### 이름 변경

탭이나 세션을 더블클릭하거나 F2를 누릅니다. 이름을 비우면 자동 이름(셸이 보내는 제목)으로 돌아갑니다.

### 명령 팔레트

`⌘K`(Windows/Linux는 `Ctrl+Shift+K`)로 엽니다. 실행, 탭·세션 이동, 이름 변경, 최대화, 패널 옮기기, 폰트 크기 명령이 있습니다.
URL을 입력하면 terminal-browser로 바로 엽니다.

### 패널 배치와 크기

탭마다 패널을 **분할 트리**로 배치합니다(타일링 창 관리자, VS Code 편집기 분할과 같은 방식).

- **도킹**: 패널 헤더를 끌어 다른 패널 위로 가져가면 놓일 자리가 표시됩니다.
  가장자리(왼쪽·오른쪽·위·아래)에 놓으면 그 패널을 반으로 나누어 그쪽에 끼워 넣고, 가운데에 놓으면 두 패널의 자리를 바꿉니다.
- **새 패널**: 포커스된 패널을 긴 쪽 방향으로 나누어 엽니다.
- **크기**: 나뉜 경계선을 끌면 그 경계의 두 패널만 크기가 바뀝니다. 화살표 키로 미세 조정하고, 더블클릭이나 Enter로 그 분할을 균등하게 되돌립니다.
- 배치와 비율은 탭마다 저장됩니다. 분할 트리 이전에 저장한 레이아웃은 같은 모양의 트리로 바꿔서 불러옵니다.

## 세션 유지

| 상황 | 동작 |
|---|---|
| 새로고침, 탭 닫았다 열기 | 같은 셸 프로세스에 다시 연결하고 화면을 재생합니다(`다시 연결됨`). |
| 대시보드 프로그램 재시작 | 같은 셸 프로세스에 다시 연결합니다. |
| 컴퓨터 재시작 | 같은 폴더에서 새 셸을 시작하고 이전 화면을 복원합니다(`복원됨`). 실행 중이던 프로그램은 되살릴 수 없습니다. |

- **PTY 호스트**(`westerm host`): 셸을 붙잡고 있는 백그라운드 프로세스입니다. 대시보드가 처음 시작할 때 같은 실행 파일로 띄우고(별도 세션, `setsid`), 대시보드가 꺼져도 남습니다.
  `~/.local/state/westerm/host.sock`(권한 0600, 폴더 0700)으로만 통신합니다.
  이미 응답하는 호스트가 있으면 새 호스트는 소켓을 지우지 않고 시작을 거부합니다.
- 셸과 SSH는 호스트 안의 화면 사본(`vt100`)에 출력을 기록하고, 다시 연결할 때 그 화면(스크롤 기록, 색, 커서, 입력 모드)을 보냅니다.
  화면 내용(최대 1,000줄)과 작업 폴더는 출력 3초 뒤, 그리고 종료·로그아웃·재부팅(SIGTERM) 때 저장합니다.
- terminal-browser와 terminal-code는 화면을 Kitty 이미지로 그리므로 화면을 저장할 수 없습니다. 다시 연결할 때 앱을 다시 시작하고, 두 앱이 자기 탭과 폴더를 스스로 복원합니다.
- 레이아웃(탭, 패널 순서, 이름, 크기 비율)은 `~/.local/state/westerm/workspace.json`에 저장합니다.
- 패널이나 탭을 닫으면 프로세스를 끝내고 저장된 화면을 지웁니다. 30일 동안 쓰지 않은 저장 파일은 지웁니다.
- 연결이 끊기면 0.5초부터 최대 8초 간격으로 8번 다시 연결합니다. 다른 창에서 같은 세션을 열면 이전 창은 자동으로 다시 연결하지 않습니다.
- 호스트 코드(`native/build.rs`의 `HOST_SOURCES`)가 바뀌면 대시보드가 호스트를 교체합니다(`westerm --version`에 호스트 빌드 ID가 나옵니다).
  이때 셸은 `복원됨`으로 다시 시작합니다. 화면이나 대시보드 서버 코드만 바꾸면 셸은 그대로입니다.
- 이름을 바꾸기 전(`wterm-dashboard`)의 상태 폴더는 처음 실행할 때 `~/.local/state/westerm`으로 옮깁니다. 그 폴더에서 돌던 호스트는 저장 후 종료되고, 셸은 한 번 `복원됨`으로 다시 시작합니다.
- 호스트를 직접 끄려면: `curl --unix-socket ~/.local/state/westerm/host.sock -X POST http://host/shutdown`

## SSH 원격 연동

- SSH 아이콘이나 팔레트의 `SSH 연결…`을 누르면 `~/.ssh/config`(Include 포함)의 호스트 목록이 나옵니다. `user@host`를 직접 입력해도 됩니다.
- **원격 셸**: 패널에서 `ssh <호스트>`를 실행합니다. 일반 셸처럼 새로고침과 프로그램 재시작에도 연결이 유지됩니다.
- **원격 브라우저**: `terminal-browser --ssh <호스트>`로 엽니다. 페이지 요청이 원격 서버를 거치므로 `localhost:3000`은 원격 서버의 개발 서버입니다.
- 인증은 기존 SSH 키와 agent를 씁니다. 비밀번호를 묻는 서버는 터미널에서 직접 입력합니다. 대시보드는 비밀번호를 저장하거나 전송하지 않습니다.
- 접속 대상은 셸 문자열이 아니라 인자로 넘기고, `-`로 시작하거나 공백·셸 문자가 있는 값은 거부합니다.

## 폰트와 테마

- 서버가 Ghostty 설정의 `font-family`와 `font-size`를 읽습니다(`/api/terminal-config`).
- Ghostty는 Nerd Font 아이콘을 내장하지만 브라우저에는 없습니다. 그래서 사용자 폰트 뒤에 설치된 Nerd Font를 fallback으로 붙입니다.
- 밝은/어두운 테마는 OS 설정을 자동으로 따릅니다. 터미널 기본 색도 함께 바뀌며(`WTerm.setThemeColors`), OSC 11로 배경색을 묻는 앱도 맞는 테마를 고릅니다.

## 보안

- 서버는 `127.0.0.1`에서만 수신합니다.
- 레이아웃 파일은 서버가 크기(256 KiB)와 기본 형식만 확인하고, 자세한 검증은 화면이 불러올 때 합니다.
- WebSocket과 레이아웃 저장(PUT)은 대시보드 자신의 Origin만 허용합니다. 다른 사이트는 셸을 열거나 레이아웃을 바꿀 수 없습니다.
- 세션 ID, 종류, 크기, URL, SSH 대상은 서버와 호스트 양쪽에서 검증합니다.

## wterm과 terminal-browser 호환 보정

- **대체 화면 이미지**(`src/terminal/alt-screen-images.ts`): terminal-browser는 전체 프레임을 크기 지정 없는 Kitty 이미지(`a=T,...,C=1`)로 보냅니다.
  `@wterm/dom` 0.5.9는 이를 셸 출력 속 이미지로 보고 아래 줄을 밀어냅니다. 대체 화면에서만 이 동작을 끕니다.
- **Cmd 단축키**(`src/terminal/command-keys.ts`): `@wterm/dom`은 Cmd+키를 모두 브라우저에 맡깁니다. 앱이 Kitty "모든 키 보고"(flag 8)를 켰을 때만 Cmd 조합을 앱에 보냅니다.
  Cmd+V는 브라우저에 남깁니다. Cmd+T, Cmd+W, Cmd+N, Cmd+Q는 Chrome이 먼저 가져갑니다.
- 둘 다 wterm 내부 구조를 이용합니다. 구조가 바뀌면 보정이 꺼지고 콘솔에 경고가 남습니다.

## 알려진 제한

- **영상 끊김**: 브라우저는 공유 메모리를 읽을 수 없어서, terminal-browser가 프레임을 base64 텍스트로 PTY에 보냅니다. 영상처럼 화면이 계속 바뀌면 끊깁니다.
- **terminal-browser 주소창 한글**: terminal-browser 자체 UI 폰트(JetBrains Mono)에 한글이 없어서 □로 보입니다. 페이지 본문은 정상입니다.

## 테스트

```bash
npm test           # 화면 로직(node:test) + Rust 단위·통합 테스트(cargo test)
npm run type-check
```

Rust 통합 테스트(`native/tests/integration.rs`)는 실제 실행 파일을 임시 상태 폴더에서 띄워서 확인합니다.

- 연결이 끊겨도 같은 셸에 다시 연결되고, 호스트가 재시작되면 저장된 화면으로 복원되며, 닫으면 저장 파일이 지워짐
- 두 번째 창이 같은 세션을 열면 첫 창이 4001 코드로 알림을 받음
- 대시보드는 자기 출처의 WebSocket만 받음
- 이미 실행 중인 호스트가 있으면 새 호스트가 시작을 거부함

```bash
```
