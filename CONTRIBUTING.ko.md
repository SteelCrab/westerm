# westerm 기여 안내

[English](CONTRIBUTING.md) · **한국어**

도와주셔서 고맙습니다. 이 문서는 개발 준비, PR 전 확인 사항, 프로젝트 규칙을 설명합니다.

westerm은 [MIT 라이선스](LICENSE)입니다. 기여를 보내면 같은 조건으로 라이선스하는 데 동의하는 것으로 봅니다.

## 기여 방법

- **버그 제보**: 버그 제보 양식으로 [이슈](https://github.com/SteelCrab/westerm/issues)를 엽니다. [버그 제보](#버그-제보)를 참고하세요.
- **아이디어 제안**: 먼저 기능 제안 양식으로 이슈를 엽니다. 큰 변경은 코드를 쓰기 전에 방향을 합의합니다. [기능 제안](#기능-제안)을 참고하세요.
- **수정과 기능 추가**: PR을 보냅니다.
- **번역**: 화면 언어를 다듬거나 새 언어를 추가합니다. [화면 문구와 언어](#화면-문구와-언어)를 참고하세요.

## 개발 준비

필요한 것:

- Node.js 24 이상
- Rust (stable, cargo 포함)
- macOS, Linux, Windows 중 하나. CI가 세 곳 모두에서 테스트를 돌립니다.
- 선택: 아이콘용 프로그램 `terminal-browser`(`brew install --cask terminal-browser`), `tode`(`curl -fsSL https://tode.sh/install | bash`)

```bash
git clone https://github.com/SteelCrab/westerm.git
cd westerm
npm install
npm run build      # 화면(Vite) 빌드 → 화면을 넣은 Rust 실행 파일 빌드
npm start          # http://127.0.0.1:4317
```

### 개발 서버

```bash
npm run dev:server   # Rust 서버 :4317, Vite 주소를 허용 출처로 추가
npm run dev          # Vite HMR :5173, /api와 /ws를 :4317로 전달
```

http://localhost:5173 을 엽니다. 화면 코드는 바로 반영됩니다. Rust 코드를 바꾸면 `dev:server`를 다시 시작합니다.

### 내 세션 보호하기

westerm은 셸을 백그라운드 **PTY 호스트**에 둡니다. 호스트와 상태 파일은 기본으로 `~/.local/state/westerm`에 있습니다. westerm을 평소에도 쓴다면 개발 빌드가 그 세션을 건드릴 수 있습니다.

- 호스트 코드를 바꾼 개발 서버는 **실행 중인 호스트를 교체**합니다. 그러면 셸이 "복원됨"으로 다시 시작합니다.
- 같은 세션을 연 두 번째 브라우저 창은 첫 창의 세션을 가져갑니다.

개발할 때는 상태 폴더와 포트를 따로 씁니다.

```bash
WESTERM_STATE_DIR=/tmp/westerm-dev ./native/target/release/westerm --port 4318
```

```powershell
$env:WESTERM_STATE_DIR = "$env:TEMP\westerm-dev"; .\native\target\release\westerm.exe --port 4318
```

- macOS와 Linux에서는 경로를 짧게 씁니다. 호스트 소켓 경로는 104바이트 안이어야 합니다(Unix 소켓 제한).
- `npm run dev`는 항상 :4317로 전달합니다. 평소 쓰는 대시보드를 먼저 끕니다(Ctrl+C). 셸은 호스트 안에 그대로 남습니다.
- 시험용 호스트는 같은 `WESTERM_STATE_DIR`로 `westerm stop`을 실행해 끄고, 폴더를 지웁니다.

### 개발 빌드로 원격 창 시험하기

소스 빌드의 코드는 릴리스에 없습니다. 그래서 원격 창은 직접 만든 Linux 빌드를 서버에 설치합니다. 두 빌드는 같은 소스에서 만듭니다. westerm이 양쪽의 버전 줄을 비교하기 때문입니다.

```bash
brew install zig && cargo install cargo-zigbuild   # 한 번만 (zig가 Linux 빌드를 링크합니다)
npx vite build
cargo build --release --manifest-path native/Cargo.toml
cargo zigbuild --release --target x86_64-unknown-linux-musl --manifest-path native/Cargo.toml
mkdir -p /tmp/wrb
cp native/target/x86_64-unknown-linux-musl/release/westerm /tmp/wrb/westerm-x86_64-unknown-linux-musl
WESTERM_STATE_DIR=/tmp/westerm-dev WESTERM_REMOTE_BINARY_DIR=/tmp/wrb ./native/target/release/westerm --port 4318
```

- Arm 서버에는 `aarch64-unknown-linux-musl`을 씁니다.
- `WESTERM_REMOTE_TRANSPORT=tunnel`로 SSH 터널 경로를 시험합니다.
- `native/tests/remote.rs`는 가짜 `ssh`로 전체 경로를 시험합니다. 서버가 필요 없습니다.

## 프로젝트 구조

| 경로 | 내용 |
|---|---|
| `src/` | 화면: React + wterm (Ghostty WASM) |
| `src/i18n/` | 모든 언어의 화면 문구, 언어 감지 |
| `src/terminal/` | 터미널 뷰, WebSocket 연결, 글꼴, wterm 보정 |
| `shared/` | 프로토콜 타입, 레이아웃 트리, 저장 파일 검증 |
| `native/src/server/` | 대시보드 서버(axum): 화면 제공, API, 소켓을 호스트로 중계 |
| `native/src/host/` | PTY 호스트: 프로세스 실행·유지, 화면 사본(`vt100`), 저장·복원 |
| `native/src/protocol.rs` | 메시지 형식과 검증 |
| `native/src/i18n.rs` | 호스트가 터미널에 쓰는 문구의 모든 언어 |
| `native/src/ipc.rs` | 대시보드 ↔ 호스트 통로: Unix 소켓, Windows는 named pipe |
| `native/src/remote/` | 서버 쪽 원격 창: `westerm gate`, `westerm remote-setup`, `westerm remote-dashboard`, 인증서, QUIC 설정 |
| `native/src/remote/daemon.rs` | gate와 서버 대시보드가 함께 쓰는 것: 하나만 실행, 실행 기록, 멈추기 |
| `native/src/remote/dashboard.rs` | `westerm remote-dashboard`: 서버의 대시보드. 비공개 Unix 소켓에서만 듣습니다 |
| `native/src/server/remote/` | 로컬 대시보드 쪽 원격 창: SSH 준비, 설치, QUIC과 터널 연결 |
| `native/src/server/remote_window.rs` | `/remote/<target>/…`: 원격 창의 요청을 서버 대시보드로 보내는 경로 |
| `native/src/server/http1.rs` | 아무 바이트 스트림 위의 HTTP/1.1 요청 하나: PTY 호스트로, 또는 서버로 가는 연결 위로 |
| `src/remoteWindow.ts` | 원격 창의 주소(`/?remote=<target>`), 서버 경로, 원격 창 열기 |
| `src/useRemoteLink.ts` | 로컬 대시보드의 상태 흐름에서 읽는 원격 창의 연결 상태 |
| `src/components/RemoteConnect.tsx` | 원격 창의 연결 화면과, 연결이 끊긴 동안의 안내 띠 |
| `tests/` | 화면 테스트(`node:test`) |
| `native/tests/` | 실제 실행 파일로 돌리는 Rust 통합 테스트 |
| `macos/` | macOS 앱: Swift 창(`app/main.swift`), 아이콘, `build-app.sh` |
| `scripts/start.mjs` | 모든 OS에서 `npm start` |
| `docs/` | 사용법, 세션, 구조 (영어) |

각 부분의 관계는 [docs/architecture.md](docs/architecture.md)를 보세요.

## 이슈

모든 이슈는 양식으로 시작합니다: **Bug report**(버그 제보) 또는 **Feature request**(기능 제안). 빈 이슈는 열 수 없습니다.

- 먼저 열린 이슈와 닫힌 이슈를 검색합니다. 같은 이슈가 있으면 새로 열지 말고 그 이슈에 내용을 더합니다.
- 이슈 하나에는 문제나 아이디어 하나만 씁니다.
- 제목은 무엇이 잘못됐는지, 또는 무엇을 원하는지 짧게 씁니다: `Bug`나 `Help`가 아니라 `Restored pane loses its folder on Windows`.
- 보안 문제는 공개 이슈로 올리지 않습니다. [보안 문제](#보안-문제)를 참고하세요.

### 버그 제보

양식은 다음을 묻습니다.

- `westerm --version` 출력(호스트 빌드 ID 포함)
- 실행 방법: 설치 스크립트, 릴리스 압축 파일, macOS 앱, 소스 빌드
- OS와 브라우저, 각 버전
- 재현 순서, 기대한 결과, 실제 결과
- 호스트 로그와 브라우저 콘솔의 관련 줄. 로그는 macOS·Linux에서 `~/.local/state/westerm/host.log`, Windows에서 `%LOCALAPPDATA%\westerm\host.log`입니다.

호스트 이름, 경로, 화면 내용 같은 개인 정보는 먼저 지워 주세요.

### 기능 제안

- 문제를 먼저 쓰고, 그다음 제안을 씁니다. 해결 방법은 논의하면서 바뀔 수 있습니다.
- 큰 변경은 코드를 쓰기 전에 이슈에서 방향을 합의합니다.

## PR

- 작은 수정이 아니면 먼저 이슈를 열고 방향을 합의합니다.
- `main`에서 브랜치를 만듭니다. PR 하나에는 주제 하나만 담습니다.
- 제목은 `<type>(<scope>): <subject>` 형식으로, 전체 약 72자 이하로 씁니다.
  - `type`: `feat`(새 기능), `fix`(버그 수정), `docs`, `refactor`, `test`, `chore`(빌드, CI, 릴리스).
  - `scope`: 바뀌는 부분. 예: `remote`, `host`, `ui`, `i18n`, `macos`.
  - `subject`: 명령형 영어, 소문자로 시작, 끝에 마침표 없음.
  - 예: `feat(remote): install westerm over SSH and connect over QUIC`, `fix(host): keep the folder of a restored shell`.
- 템플릿을 채웁니다: 한 줄짜리 항목 5개 이하의 요약, 그리고 리뷰어가 테스트할 때 실행할 명령입니다. `Closes #123`으로 이슈를 연결하면 병합할 때 이슈가 닫힙니다.
- 작업 중에는 draft로 엽니다. [PR 전 확인](#pr-전-확인)을 통과하면 ready로 바꿉니다.
- CI는 macOS, Linux, Windows에서 모두 통과해야 합니다. CI는 타입 검사, 화면 테스트, 화면 빌드를 한 번 돌리고, 각 OS에서 clippy와 `cargo test`를, Linux에서 릴리스 빌드를 돌립니다. 셸 스크립트는 shellcheck로, macOS 앱의 Swift 코드는 타입 검사로 확인합니다.
- 리뷰 의견에는 새 커밋으로 답합니다. 리뷰 중에는 force-push하지 않습니다. 리뷰어가 바뀐 부분을 볼 수 있어야 합니다.
- 리뷰가 끝나면 메인테이너가 병합합니다.

## PR 전 확인

```bash
npm test             # 화면 테스트 + cargo test(단위·통합)
npm run type-check
npm run build        # 릴리스 빌드가 성공해야 합니다
cargo clippy --all-targets --manifest-path native/Cargo.toml -- -D warnings
```

네 명령이 모두 통과해야 합니다. 그다음 확인합니다.

- [ ] PR 하나에 주제 하나. 관계없는 변경은 나눕니다.
- [ ] 새 동작에는 테스트가 있습니다. 버그 수정에는 수정 전에 실패하는 테스트가 있습니다.
- [ ] 화면 문구는 `src/i18n/messages.ts`에 4개 언어로 모두 있습니다.
- [ ] 화면을 바꿨으면 PR에 스크린샷을 넣습니다. 밝은 테마와 어두운 테마를 모두 확인합니다.
- [ ] 문서가 변경과 맞습니다. 동작은 `docs/`, 실행·개발 방법은 README입니다.
- [ ] 사용자가 알아챌 변경이면 `CHANGELOG.md`의 `[Unreleased]` 아래에 한 줄이 있습니다.

## 규칙

### 화면 문구와 언어

- 사용자에게 보이는 문구는 모두 `src/i18n/messages.ts`에 넣습니다. 컴포넌트에 직접 쓰지 않습니다.
- 기준은 `en`입니다. 각 언어는 `Messages` 타입이라서, 키가 빠지면 타입 검사가 실패합니다. `tests/i18n.test.ts`도 모든 언어를 `en`과 비교합니다.
- 값이 들어가는 문구는 함수로 씁니다. 예: `sessionCount: (count) => ...`. 조각을 이어 붙여 문장을 만들지 않습니다. 언어마다 어순이 다릅니다.
- 명령 팔레트 항목은 영어 `keywords`를 유지합니다. 어느 언어에서든 영어로 찾을 수 있게 하기 위해서입니다.
- 호스트가 터미널에 쓰는 문구(복원 안내, 오류)는 `native/src/i18n.rs`에 있습니다. 화면 언어와 맞춥니다.
- 번역이 확실하지 않으면 PR에 적어 주세요. 그 언어를 아는 리뷰어가 확인합니다.

새 언어 추가 순서:

1. `src/i18n/messages.ts`: `Locale`, `LOCALES`, `LOCALE_NAMES`, `HTML_LANG`에 추가하고, 문구 목록을 쓰고, `MESSAGES`에 넣습니다.
2. `npm run type-check`가 나머지 고칠 곳을 알려 줍니다: 팔레트 언어 키워드, CJK 글꼴 순서(`src/terminal/fonts.ts`).
3. `native/src/i18n.rs`: `Lang`에 값을 추가하고 각 `match`를 채웁니다. 컴파일러가 위치를 알려 줍니다.
4. 선택: `src/styles.css`의 화면 글꼴 목록(`:root:lang(xx)`), 번역한 README.

### 브라우저 ↔ 서버 프로토콜

- `shared/protocol.ts`(브라우저)와 `native/src/protocol.rs`(서버와 호스트)는 같은 메시지를 설명합니다. 함께 바꿉니다.
- 서버와 호스트는 모든 연결 요청을 각각 검증합니다: 세션 ID, 종류, 크기, URL, SSH 대상, 언어. 양쪽 검증을 유지합니다.

### PTY 호스트

- 호스트 빌드 ID는 `HOST_SOURCES`(`native/build.rs`) 파일들의 해시입니다. ID가 바뀌면 대시보드가 실행 중인 호스트를 교체하고, 모든 셸이 한 번 "복원됨"으로 다시 시작합니다.
- 새 호스트 파일은 `HOST_SOURCES`에 넣습니다. 호스트 코드는 꼭 필요할 때만 바꿉니다. 화면과 대시보드 서버 변경은 셸을 다시 시작하지 않습니다.

### 보안

- 서버는 `127.0.0.1`에서만 수신합니다. WebSocket과 레이아웃 저장은 대시보드 자신의 Origin만 받습니다. 이 검사를 약하게 만들지 않습니다.
- 사용자 입력을 셸 문자열로 넘기지 않습니다. `spawn.rs`가 SSH 대상과 URL을 다루듯 인자로 넘깁니다.
- 상태 파일과 호스트 소켓은 사용자만 접근합니다(폴더 0700, 파일 0600).

### 플랫폼

westerm은 macOS, Linux, Windows에서 동작합니다. CI가 모든 PR을 세 곳에서 시험합니다.

- OS별 코드는 `#[cfg(unix)]`와 `#[cfg(windows)]`로 나누고, 모든 OS에 동작을 마련합니다. 차이는 작은 모듈에 모읍니다: `ipc.rs`(호스트 통로), `spawn.rs`(세션이 실행하는 것), `state_dir.rs`(폴더와 파일 권한), `cwd.rs`, `time.rs`.
- macOS와 Linux는 로그인 셸을 거쳐 세션을 시작합니다. Windows는 인자 목록으로 프로그램을 바로 실행합니다. `.cmd` 실행 파일은 인자를 cmd.exe로 넘기므로, 인자에 cmd.exe 문법 문자가 없는지 확인합니다.
- push 전에 macOS나 Linux에서 Windows 빌드를 확인합니다.

  ```bash
  rustup target add x86_64-pc-windows-msvc
  cargo check --target x86_64-pc-windows-msvc --all-targets --manifest-path native/Cargo.toml
  ```

- macOS에서 Docker로 Linux 테스트를 돌립니다.

  ```bash
  docker run --rm -v "$PWD":/src:ro -w /src/native -e CARGO_TARGET_DIR=/tmp/target rust:1 cargo test --locked
  ```

### wterm 보정

`src/terminal/alt-screen-images.ts`와 `src/terminal/command-keys.ts`는 wterm 내부 구조를 씁니다. wterm이 바뀌면 보정은 스스로 꺼지고 경고를 남겨야 합니다. 터미널을 망가뜨리면 안 됩니다. PR에 시험한 wterm 버전을 적어 주세요.

### 코드 스타일

아직 포매터 설정은 없습니다. 바꾸는 곳 주변 코드에 맞춥니다.

- TypeScript: strict 모드, 2칸 들여쓰기, 큰따옴표, 세미콜론, 끝 쉼표, 한 줄 약 120자까지.
- React: 함수 컴포넌트. 앱은 `StrictMode`를 쓰지 않습니다. 이펙트가 두 번 실행되면 패널마다 셸을 하나 더 띄웠다 끄기 때문입니다.
- Rust: 4칸 들여쓰기, 한 줄 약 120자까지. 서버와 호스트에서는 panic 대신 오류를 반환합니다.
- 주석은 무엇을 하는지보다 왜 그렇게 하는지를 씁니다.

### 커밋

- 제목은 명령형 영어, 약 72자 이하: `added ...`가 아니라 `Add Japanese restore notice`.
- 이유가 분명하지 않으면 본문에 왜 바꾸는지 씁니다.
- PR 제목은 형식이 다릅니다: [PR](#pr)을 보세요.

## 릴리스

메인테이너용입니다. 버전 태그를 올리면 빌드와 배포가 모두 진행됩니다.

1. `package.json`과 `native/Cargo.toml`에 같은 버전을 넣습니다.

   ```bash
   npm version 0.2.0 --no-git-tag-version     # package.json, package-lock.json
   # native/Cargo.toml의 version을 고친 뒤 Cargo.lock 갱신:
   cargo check --manifest-path native/Cargo.toml
   ```

2. 커밋하고 push한 뒤, CI가 세 OS에서 통과하는지 확인합니다.
3. 태그를 만들고 push합니다.

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

4. [Release 워크플로](.github/workflows/release.yml)가 진행합니다.
   - 태그와 두 버전이 같은지 확인합니다.
   - macOS(Apple Silicon, Intel), Linux(x64, arm64, 정적 빌드), Windows를 시험하고 빌드합니다.
   - macOS 앱(공용 빌드)을 묶습니다.
   - 압축 파일과 SHA-256 값을 GitHub 릴리스로 올립니다. `install.sh`와 `install.ps1`이 이 릴리스를 최신 버전으로 설치합니다.
   - 마지막으로 모든 OS에서 `install.sh`, `install.ps1`로 릴리스를 실제 설치해 확인합니다.

PTY 호스트 코드가 바뀐 버전은 사용자의 셸을 한 번 "복원됨"으로 다시 시작합니다. 릴리스 노트에 적어 주세요.

## 보안 문제

공개 이슈로 올리지 마세요. [비공개 취약점 제보](https://github.com/SteelCrab/westerm/security/advisories/new)를 사용하세요.
