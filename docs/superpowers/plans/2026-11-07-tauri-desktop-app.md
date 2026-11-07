# Tauri 데스크톱 앱 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** macOS 전용 Swift 앱(`macos/`)을 Tauri 2 앱(`desktop/`) 하나로 바꾼다. 같은 코드로 macOS, Windows, Linux 앱을 만든다.

**Architecture:** 앱은 지금 Swift 앱과 같은 얇은 창이다. `127.0.0.1:4317`에 westerm이 있으면 그 대시보드를 열고, 없으면 앱에 함께 넣은 `westerm` 실행 파일(Tauri sidecar)로 `westerm serve`를 시작한다. 페이지와 앱은 `window.westermApp.request(message)` 하나로 이야기한다(Tauri IPC, 원격 출처 capability). 브라우저 모드는 그대로다.

**Tech Stack:** Tauri 2.12 (tauri, tauri-build, tauri-plugin-opener, tauri-plugin-single-instance, tauri-plugin-window-state), Rust 2024, `@tauri-apps/cli` 2.12, TypeScript/React, node:test, cargo test.

**Spec:** 별도 스펙 없음. 사용자 요청: "Linux, Windows, macOS 앱 개발의 통일성을 위해 westerm을 Tauri로 리팩터링". 질문 없이 진행하기로 했다. 아래 결정은 모두 구현자(Claude)의 판단이다.

## 결정 (구현자 판단)

- Tauri 2 안정판을 쓴다. 3.0은 알파다.
- 앱 크레이트는 `desktop/`에 둔다. `native/`(서버)와 Cargo.lock, target을 따로 쓴다.
- 서버를 앱 프로세스 안에 넣지 않는다. PTY host가 앱 종료 뒤에도 살아야 하고, host는 `westerm host`로 따로 실행된다.
- 앱이 시작한 서버만 앱이 끈다. host와 셸은 남는다(지금과 같음).
- 업데이트는 서명 키가 필요한 `tauri-plugin-updater` 대신 지금처럼 릴리스의 SHA-256 합으로 확인한다. 새 GitHub secret이 필요 없다.
  - macOS: `westerm-macos-app.zip`을 받아 앱 번들을 바꾸고 다시 연다. 이름이 같아서 기존 Swift 앱도 이 zip으로 Tauri 앱으로 업데이트된다.
  - Windows: `westerm-windows-app-setup.exe`(NSIS)를 받아 확인하고, `westerm stop`으로 host를 멈춘 뒤 설치 파일을 실행하고 앱을 끝낸다. 실행 중인 `westerm.exe`는 덮어쓸 수 없기 때문이다.
  - Linux: `.deb`, `.rpm`은 root 없이 바꿀 수 없다. 릴리스 페이지를 브라우저로 연다.
  - AppImage는 만들지 않는다. 앱이 끝나면 AppImage의 마운트가 사라지고, 거기서 실행된 host와 셸도 위험해진다.
- 업데이트 확인과 화면은 페이지의 업데이트 창 하나로 통일한다. macOS 메뉴의 "업데이트 확인…"은 페이지에 `westerm:update` 이벤트를 보낸다.
- 메뉴 막대는 macOS에만 둔다. Windows와 Linux는 페이지 단축키(Ctrl+Shift)를 쓴다. ⌘ 단축키(⌘1–9 등)는 macOS 앱에서만 켠다.
- 업데이트 채널 `mac-app`을 `app`으로 바꾼다. 실행 파일 옆에 `westerm-app`(Windows: `westerm-app.exe`)이 있거나 경로가 `.app/Contents/MacOS/` 안이면 `app`이다.
- 로그: Tauri의 `app_log_dir()/westerm.log` (macOS: `~/Library/Logs/io.github.steelcrab.westerm/westerm.log`).

## 리뷰 후 바꾼 점

- 앱 안의 업데이트 창은 서버가 아니라 앱에 묻는다(`{type:"updateStatus", refresh}`). 앱이 터미널에서 띄운 westerm을 보여 줄 때도 앱 자신을 업데이트할 수 있다.
- universal 빌드는 `westerm-aarch64-apple-darwin`, `westerm-x86_64-apple-darwin`, `westerm-universal-apple-darwin`을 모두 만든다. Tauri가 CPU마다 따로 빌드하며 각자의 파일을 찾기 때문이다.
- Windows: 서버를 먼저 끄고(바로 kill) `westerm stop`을 부른다. 서버가 살아 있으면 다시 붙는 패인 때문에 host가 다시 시작된다. 설치·제거 전에도 NSIS 훅(`desktop/windows/hooks.nsh`)이 host를 멈춘다.
- 업데이트가 서버를 끈 뒤 실패하면 대시보드를 다시 찾거나 시작한다.
- 새 창 요청(`on_new_window`)은 다른 스레드에서 처리한다. Linux 번들은 `deb`, `rpm`만 만든다(`tauri.linux.conf.json`).

## Global Constraints

- 식별자 `io.github.steelcrab.westerm`, 제품 이름 `westerm`, 앱 실행 파일 `westerm-app`, sidecar `westerm`.
- 앱 버전은 `package.json`에서 읽는다(`"version": "../package.json"`). `desktop/Cargo.toml`도 같은 버전. 릴리스 `check` 잡이 셋을 비교한다.
- macOS 최소 버전 12.0. Info.plist에 `NSAllowsLocalNetworking`, `NSLocalNetworkUsageDescription`을 그대로 넣는다.
- capability는 `http://127.0.0.1:*`의 `app_request` 명령만 허용한다. 앱도 명령을 받을 때 요청한 페이지가 대시보드 출처인지 확인한다.
- 대시보드 출처 밖의 주소는 창에서 열지 않고 기본 브라우저로 연다.
- UI 문구는 `src/i18n/messages.ts`에 en, ko, ja, zh. 앱 메뉴와 시작 화면 문구도 같은 네 언어.
- 레포 문서(`docs/`, `CHANGELOG.md`)는 영어.
- 에이전트는 `git commit`/`git push`를 실행하지 않는다. 커밋 명령은 제안만 한다.
- 개발 중 사용자의 westerm(4317)을 멈추지 않는다. 앱 수동 실행은 `PORT=4319 WESTERM_STATE_DIR=<scratch>`.

## Review Focus

1. 4317에 westerm이 아닌 다른 프로그램이 있을 때: 앱은 그 프로그램을 열지 않고 빈 포트로 자기 서버를 시작해야 한다. → Task 3 `server::tests::port_zero_is_tried_after_the_preferred_port` 대신 수동 확인 + `probe_rejects_non_westerm_answers`.
2. 원격 대상 이름에 `@`, `.`, `:` 같은 문자가 있을 때: 창 이름(label)이 유효하고, 같은 서버는 같은 창으로 돌아와야 한다. → Task 3 `remote_label_is_stable_and_valid`.
3. 대시보드가 아닌 페이지가 앱 명령을 부를 때: 거부해야 한다. → Task 3 `requests_from_other_origins_are_refused`.
4. 업데이트 파일이 SHA-256 합과 다를 때: 아무것도 바꾸지 않아야 한다. → Task 5 `checksum_mismatch_is_refused`.
5. 브라우저에서 앱이 시작한 서버를 열 때: 업데이트 버튼은 "앱에서 업데이트하세요"를 보여야 한다. → Task 2 `UpdateButton` 문구 테스트(채널 `app`).

---

### Task 1: 서버의 업데이트 채널 `app`

**Files:** Modify `native/src/update.rs`, `native/src/server/remote/binary.rs`, `shared/protocol.ts`

- [ ] `Channel::MacApp` → `Channel::App` (`"app"`). `channel_of(path)`: `.app/Contents/MacOS/` 안이거나 `path.with_file_name(APP_EXECUTABLE)`이 있으면 `App`.
- [ ] 테스트 `channel_of_detects_the_desktop_app`: 임시 폴더에 `westerm`과 `westerm-app`을 만들면 `App`, `westerm`만 있으면 `Binary`.
- [ ] `install()` 오류 문구: "this westerm is inside the westerm app; update the app".
- [ ] `shared/protocol.ts`: `UpdateChannel = "binary" | "app" | "source"`.
- [ ] `cargo test --manifest-path native/Cargo.toml update::` 통과.

### Task 2: 페이지 ↔ 앱 연결

**Files:** Create `src/appBridge.ts`; Modify `src/useUpdate.ts`, `src/remoteWindow.ts`, `src/App.tsx`, `src/paletteCommands.tsx`, `src/components/UpdateButton.tsx`, `src/i18n/messages.ts`; Test `tests/appBridge.test.ts`, `tests/remote.test.ts`

**Interfaces (Produces):**
- `type AppMessage = { type: "update" } | { type: "remoteWindow"; target: string }`
- `type UpdateResult = "relaunching" | "manual"`
- `interface AppBridge { request(message: AppMessage): Promise<unknown> }`
- `appBridge(): AppBridge | null` — `window.westermApp`이 있으면 그것.
- `appCommandKeys(bridge, mac): boolean` — ⌘ 단축키는 macOS 앱에서만.
- 페이지 이벤트: `westerm:command`(기존), `westerm:settings`(기존), `westerm:update`(새로, 확인 후 업데이트 창 열기).

- [ ] 테스트: `appCommandKeys`는 (앱, mac)일 때만 true. `openRemoteWindow`는 bridge가 있으면 `request({type:"remoteWindow", target})`.
- [ ] `useUpdate.install()`: 채널 `app`이면 phase `installing` → `request({type:"update"})` 결과가 `relaunching`이면 `restarting`, `manual`이면 `idle`, 실패하면 `failed`와 오류 문구.
- [ ] i18n `update.app`: "이 westerm은 westerm 앱 안에서 실행 중입니다. 앱에서 업데이트하세요." (4개 언어)
- [ ] `npm run type-check`, `node --import tsx --test tests/*.test.ts` 통과.

### Task 3: Tauri 앱 뼈대 — 서버, 창, 연결

**Files:** Create `desktop/{Cargo.toml,build.rs,tauri.conf.json,Info.plist,capabilities/dashboard.json,ui/index.html,icons/*}`, `desktop/src/{main.rs,server.rs,windows.rs,bridge.rs,strings.rs}`, `scripts/desktop.mjs`

- `server.rs`: `probe(port) -> bool`(`GET /api/terminal-config`, 1초, 200 + `fontFamilies`), `dashboard_url(output) -> Option<u16>`("westerm: http://127.0.0.1:<port>" 줄이 끝난 뒤), `utf8_locale(lang) -> String`, `Server::start(port, log)`, `Server::stop()`(Unix SIGTERM, Windows kill), Windows는 `CREATE_NO_WINDOW`.
- `windows.rs`: 시작 화면(`ui/index.html?state=starting|cannot-start|stopped&log=<path>`), 대시보드 열기, `remote_label(target)`(`remote-` + 16진수), 탐색 정책 `allowed(url, port)`, 새 창 요청은 기본 브라우저로.
- `bridge.rs`: `#[tauri::command] app_request(webview, message) -> Result<Value, String>`; 출처 확인.
- 초기화 스크립트가 `window.westermApp = { request }`를 정의한다.
- `scripts/desktop.mjs`: `prepare [--release] [--target T]`(vite build, native 빌드, `desktop/binaries/westerm-<triple>`에 복사; `universal-apple-darwin`은 lipo), `dev`, `build`(tauri build, 결과물을 `dist-app/`에 릴리스 이름으로, macOS는 zip도).
- [ ] 단위 테스트: `dashboard_url_needs_a_complete_line`, `probe_rejects_non_westerm_answers`, `remote_label_is_stable_and_valid`, `navigation_stays_on_the_dashboard`, `requests_from_other_origins_are_refused`, `utf8_locale_falls_back_to_en_us`.
- [ ] `cargo test --manifest-path desktop/Cargo.toml`, `cargo clippy ... -D warnings` 통과.
- [ ] 수동: `PORT=4319` 앱 실행 → 대시보드가 열리고, 원격 창 요청과 업데이트 요청이 IPC로 들어오는지 로그로 확인.

### Task 4: macOS 메뉴

**Files:** Create `desktop/src/menu.rs`; Modify `desktop/src/strings.rs`

- [ ] Swift 메뉴와 같은 항목: 앱(정보, 업데이트 확인…, 설정… ⌘,, 가리기, 기타 가리기, 모두 보기, 종료), 편집, 보기(새로고침 ⌘R, 브라우저에서 열기, 전체 화면), 윈도우(최소화, 확대/축소, 닫기, 다음/이전 탭 ⌘⇧]/[, 다음/이전 패널 ⌘]/[, 탭으로 이동 ⌘1–9).
- [ ] 메뉴 명령은 활성 창의 페이지에 `westerm:command`, `westerm:settings`, `westerm:update`를 보낸다.

### Task 5: 앱 업데이트

**Files:** Create `desktop/src/update.rs`

- [ ] `Release::parse(json)`, `is_newer(a, b)`, `asset_name(os)`, `verify(file, sums_text)`.
- [ ] 테스트: `parses_the_latest_release`, `versions_compare_by_number`, `checksum_mismatch_is_refused`, `asset_names_per_system`.
- [ ] macOS: 번들 폴더에 쓸 수 있는지, AppTranslocation·읽기 전용 볼륨인지 확인 → 같은 폴더에 내려받기 → `ditto -x -k` → `Info.plist` 버전 확인 → 교체(실패하면 되돌림) → 서버 끄기 → `open -n` → 종료.
- [ ] Windows: 설치 파일 확인 → `westerm stop` → 서버 끄기 → 설치 파일 실행 → 종료.
- [ ] Linux: 릴리스 페이지 열기 → `manual`.

### Task 6: 빌드, CI, 릴리스, Swift 앱 삭제

**Files:** Modify `package.json`, `.gitignore`, `.github/workflows/{ci,release}.yml`; Delete `macos/`

- [ ] `npm run app:dev`, `npm run build:app`.
- [ ] CI: `macos-app` 잡 → `desktop` 잡(macOS, Linux, Windows에서 clippy + test). Linux는 webkit2gtk 4.1 패키지 설치.
- [ ] 릴리스: `app` 잡 → 매트릭스(macOS universal, Windows x64, Linux x64/arm64). 결과물: `westerm-macos-app.{zip,dmg}`, `westerm-windows-app-setup.exe`, `westerm-linux-app-{amd64,arm64}.deb`, `westerm-linux-app-{x86_64,aarch64}.rpm`, 각 `.sha256`.
- [ ] `macos/AppIcon.png`, `icon-source.png`, `make-icon.swift`, `make-dmg-background.swift`는 `desktop/assets/`로 옮긴다. DMG 배경은 미리 만든 TIFF.
- [ ] macOS에서 `npm run build:app` 성공, 앱 실행 확인.

### Task 7: 문서

- [ ] `README*.md`(4개 언어) 설치 줄, `docs/install.md`, `docs/architecture.md`, `docs/usage.md`, `docs/sessions.md`, `CONTRIBUTING*.md`, `CHANGELOG.md`(Unreleased).
