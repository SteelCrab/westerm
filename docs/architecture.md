# 구조

## 구성 요소

```
브라우저 (React + wterm Ghostty WASM)
   │  WebSocket /ws/session, HTTP /api/*
   ▼
대시보드 서버  westerm            127.0.0.1:4317, 웹 화면 내장
   │  WebSocket over Unix socket (Windows: named pipe)
   ▼
PTY 호스트    westerm host       상태 폴더의 host.sock / host.pipe
   │  PTY (Windows: ConPTY)
   ▼
셸, ssh, terminal-browser, tode
```

| 경로 | 내용 |
|---|---|
| `native/src/server/` | 대시보드 서버(axum): 화면 제공, API, 브라우저 WebSocket을 호스트로 중계 |
| `native/src/host/` | PTY 호스트: 프로세스 실행·유지(`portable-pty`), 화면 사본(`vt100`), 저장·복원 |
| `native/src/protocol.rs` | 메시지 형식과 검증(세션 ID, 크기, URL, SSH 대상, 언어) |
| `native/src/i18n.rs` | 호스트가 터미널에 쓰는 문구(복원 안내, 오류)의 4개 언어 |
| `native/src/ipc.rs` | 대시보드 ↔ 호스트 통로: Unix 소켓, Windows는 named pipe |
| `native/src/spawn.rs` | 세션이 실행하는 프로그램: macOS·Linux는 로그인 셸, Windows는 PowerShell과 인자 목록 |
| `macos/` | macOS 앱: WKWebView 창(Swift), 아이콘, 빌드 스크립트. 4317 포트의 westerm을 쓰거나 내장 실행 파일을 띄웁니다. |
| `src/` | 화면(React + wterm Ghostty WASM) |
| `src/i18n/` | 화면 문구의 4개 언어와 언어 선택 |
| `shared/` | 화면이 쓰는 프로토콜 타입, 레이아웃 트리, 저장 형식 검증 |

## 보안

- 서버는 `127.0.0.1`에서만 수신합니다.
- WebSocket과 레이아웃 저장(PUT)은 대시보드 자신의 Origin만 허용합니다.
  다른 사이트는 셸을 열거나 레이아웃을 바꿀 수 없습니다.
- 세션 ID, 종류, 크기, URL, SSH 대상, 언어는 서버와 호스트 양쪽에서 검증합니다.
- SSH 대상은 셸 문자열이 아니라 인자로 넘깁니다. `-`로 시작하거나 공백·셸 문자가 있는 값은 거부합니다.
- Windows는 `.cmd` 실행 파일이 인자를 cmd.exe로 넘깁니다. 그래서 terminal-browser로 여는 URL에 `" & | < > ^ % !`가 있으면 거부합니다.
- Windows 호스트 통로는 Unix 소켓 파일 권한 대신 다음으로 보호합니다.
  - pipe 이름은 128비트 무작위 값이고, 사용자만 읽는 상태 폴더의 `host.pipe`에만 적습니다.
  - 첫 인스턴스를 `first_pipe_instance`로 만들어서, 누가 같은 이름을 먼저 만들었으면 호스트가 시작하지 않습니다.
  - 원격 클라이언트를 거부합니다.
  - 대시보드는 연결한 pipe의 서버 PID가 `host.pipe`의 PID와 같은지 확인합니다.
- 레이아웃 파일은 서버가 크기(256 KiB)와 기본 형식만 확인합니다. 자세한 검증은 화면이 불러올 때 합니다.

## wterm과 terminal-browser 호환 보정

- **대체 화면 이미지**(`src/terminal/alt-screen-images.ts`)
  - terminal-browser는 전체 프레임을 크기 지정 없는 Kitty 이미지(`a=T,...,C=1`)로 보냅니다.
  - `@wterm/dom` 0.5.9는 이를 셸 출력 속 이미지로 보고 아래 줄을 밀어냅니다.
  - 대체 화면에서만 이 동작을 끕니다.
- **Cmd 단축키**(`src/terminal/command-keys.ts`)
  - `@wterm/dom`은 Cmd+키를 모두 브라우저에 맡깁니다.
  - 앱이 Kitty "모든 키 보고"(flag 8)를 켰을 때만 Cmd 조합을 앱에 보냅니다.
  - Cmd+V는 브라우저에 남깁니다. Cmd+T, Cmd+W, Cmd+N, Cmd+Q는 Chrome이 먼저 가져갑니다.
- 둘 다 wterm 내부 구조를 이용합니다. 구조가 바뀌면 보정이 꺼지고 콘솔에 경고가 남습니다.

## 알려진 제한

- **영상 끊김**: 브라우저는 공유 메모리를 읽을 수 없습니다.
  그래서 terminal-browser가 프레임을 base64 텍스트로 PTY에 보냅니다. 화면이 계속 바뀌면 끊깁니다.
- **Windows의 terminal-browser, terminal-code**: 두 앱은 Kitty 그래픽으로 화면을 그립니다. Windows ConPTY는 이 신호를 전달하지 않아서 아직 동작하지 않습니다. 셸과 SSH는 동작합니다.
- **terminal-browser 주소창 한글**: terminal-browser 자체 UI 폰트(JetBrains Mono)에 한글이 없어서 □로 보입니다.
  페이지 본문은 정상입니다.

## 테스트

```bash
npm test           # 화면 로직(node:test) + Rust 단위·통합 테스트(cargo test)
npm run type-check
```

CI(`.github/workflows/ci.yml`)가 macOS, Linux, Windows에서 같은 테스트를 돌립니다.
Rust 통합 테스트(`native/tests/integration.rs`)는 실제 실행 파일을 임시 상태 폴더에서 띄워서 확인합니다. Windows에서는 PowerShell 셸과 named pipe로 같은 시나리오를 시험합니다.

- 연결이 끊겨도 같은 셸에 다시 연결된다. 호스트가 재시작되면 저장된 화면으로 복원된다. 닫으면 저장 파일이 지워진다.
- 두 번째 창이 같은 세션을 열면 첫 창이 4001 코드로 알림을 받는다.
- 대시보드는 자기 출처의 WebSocket만 받는다.
- 이미 실행 중인 호스트가 있으면 새 호스트가 시작을 거부한다.
