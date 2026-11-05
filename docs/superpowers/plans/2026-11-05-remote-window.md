# 원격 창 (서버의 westerm을 새 창에서) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 로컬 westerm에서 서버를 고르면 새 창이 열리고, 그 창은 서버에 저장된 탭과 워크스페이스, 서버에서 도는 셸·terminal-browser·terminal-code를 보여 준다. v0.2.0의 원격 westerm 패인은 없앤다.

**Architecture:** 서버에서는 새 `westerm remote-dashboard`가 Unix 소켓으로만 원격 창용 경로(`/api/workspace`, `/api/ssh-hosts`, `/ws/session`)를 답한다. `westerm gate`는 인증된 QUIC 스트림을 이 소켓에 잇는다. 로컬 대시보드는 `/remote/<대상>/…` 요청을 서버마다 하나인 연결(QUIC, 안 되면 `ssh -L` 터널)로 넘기고, 창이 `/api/remote/link` 상태 흐름을 여는 동안 그 연결을 유지한다. 화면 파일은 로컬에서 주고, 주소의 `?remote=<대상>`이 페이지를 원격 창 모드로 바꾼다.

**Tech Stack:** Rust (tokio, axum 0.8.9, tokio-tungstenite 0.30, quinn 0.11.12), TypeScript/React, Swift (AppKit, WebKit), node:test, cargo test.

**Spec:** `docs/superpowers/specs/2026-11-05-remote-window-design.md`

## Global Constraints

- 서버: Linux x86_64 / aarch64, 릴리스의 musl 정적 실행 파일. 서버 쪽 명령(`gate`, `remote-setup`, 새 `remote-dashboard`)과 SSH 터널은 `#[cfg(unix)]`. Windows에서 이 명령들은 `runs on Linux and macOS only`를 stderr에 쓰고 exit 2.
- 로컬: macOS, Linux, Windows에서 `cargo build`, `cargo test`, `npm run type-check`, `npm test`가 통과해야 한다.
- 서버 대시보드는 TCP로 듣지 않는다. 소켓은 `<상태 폴더>/dashboard.sock`, 권한 0600.
- 로컬 대시보드는 `127.0.0.1`에서만 듣는다. 원격 창 경로(`/remote/…`)와 상태 흐름(`/api/remote/link`)은 출처(Origin)와 Host를 둘 다 확인한다. JSON 본문 API는 `Content-Type: application/json`도 확인한다.
- `native/src/remote/`, `native/src/server/` 코드는 `HOST_SOURCES`에 넣지 않는다. `protocol.rs`, `spawn.rs` 변경으로 host 빌드 id가 바뀌는 것은 받아들인다 (스펙 5절).
- 원격 명령은 `ssh.rs`의 고정 문자열(`PROBE`, `UPLOAD`, `SETUP`, `SETUP_REPLACE`)만 쓴다. 새 원격 명령은 만들지 않는다.
- 다운그레이드 금지: `binary::install_plan` 규칙 그대로.
- `shared/protocol.ts`와 `native/src/protocol.rs`는 같은 태스크에서 함께 바꾼다.
- UI 문구는 `src/i18n/messages.ts`에 en, ko, ja, zh 네 언어로. 팔레트 `keywords`는 영어.
- 레포 문서(`docs/`, `CHANGELOG.md`)는 영어.
- 커밋 금지: 에이전트는 `git commit`/`git push`를 실행하지 않는다. 각 태스크 끝의 "체크포인트"에서 커밋 명령을 제안만 한다. 커밋 메시지에 `Co-Authored-By` 줄을 넣지 않는다. 메시지는 레포 관례대로 명령형 영어 한 줄.
- 개발 중에는 사용자의 westerm(포트 4317, `~/.local/state/westerm`)을 건드리지 않는다. 수동 실행은 `WESTERM_STATE_DIR=/tmp/wq`와 `--port 4318`.
- 테스트 서버의 host를 교체하거나 멈추기 전, 그리고 이 작업이 만들지 않은 파일을 지우기 전에는 사용자에게 묻는다.
- Rust 테스트 실행: `cargo test --manifest-path native/Cargo.toml -- <이름> <이름>` (필터 여러 개는 `--` 뒤에).

## Review Focus

1. 원격 창의 첫 레이아웃 읽기가 연결 전이거나 실패할 때: 서버에 저장된 레이아웃을 새 셸 하나로 덮어쓰면 안 된다. → Task 6 `the first load never replaces a server layout it could not read`.
2. 같은 서버를 두 창(또는 두 컴퓨터)이 동시에 열 때: 확인과 설치는 한 번만 해야 한다. → Task 5 `two_windows_opening_at_once_set_up_once`.
3. 연결이 끊긴 동안 패인을 닫을 때: 다시 연결되면 서버의 셸도 끝나야 한다. → Task 5 `a_session_closed_while_disconnected_ends_after_reconnect`, Task 6 `a pane closed while its socket is down reports the close`.
4. 연결이 조용히 죽을 때(gate 종료, 잠자기): 알아채고 다시 붙어서 같은 셸로 돌아와야 한다. → Task 5 `a_lost_gate_comes_back_and_the_shell_is_reattached`.
5. 같은 브라우저의 다른 사이트가 원격 경로를 부를 때(CSRF, DNS rebinding): 거부해야 한다. → Task 3 `remote_window_routes_check_origin_and_host`, Task 5 `remote_apis_check_the_origin`.

## 스펙과 다른 점 (계획 단계의 판단)

- 스펙 9절은 `WESTERM_REMOTE_TRANSPORT=tunnel` 통합 테스트를 적었다. 테스트의 가짜 `ssh`는 `-L` 전달을 흉내 내지 못한다(v0.2.0도 같았다). 그래서 터널 경로는 Task 10의 종단 확인에서 본다. 이 Mac에서 띄운 개발용 대시보드는 Local Network 권한이 없어서 LAN 서버에 QUIC이 막히고, 자동으로 SSH 터널을 쓴다. 이것이 터널 확인이 된다.
- 스펙 8.4의 "저장된 `remote` 세션을 읽을 때 빼요": 레이아웃 트리를 직접 고치지 않는다. 파서는 옛 원격 패인을 같은 대상의 `ssh` 자리표시 세션으로 읽고 id를 `legacyRemoteIds`에 모은다. `hydrate`가 그 id들을 `close`와 같은 규칙으로 바로 닫는다. 패인은 화면에 붙기 전에 사라진다 (Task 4).

## File Map

| 파일 | 책임 | 태스크 |
|---|---|---|
| `native/src/remote/daemon.rs` (새) | 상태 폴더의 백그라운드 프로그램: 잠금, 기록, 멈춤, 종료 신호 | 1 |
| `native/src/remote/dashboard.rs` (새) | `westerm remote-dashboard`: 소켓, 기록, 실행 | 1 |
| `native/src/remote/gate.rs` | `Daemon` 사용 (1), 스트림을 서버 대시보드 소켓에 잇기 (2) | 1, 2 |
| `native/src/remote/setup.rs` | 서버 대시보드 준비 | 2 |
| `native/src/remote/report.rs` | `SETUP_VERSION = 2`, `dashboard` 필드 | 2 |
| `native/src/ipc.rs` | `bind_socket(path)` | 1 |
| `native/src/main.rs` | `remote-dashboard` 명령, 사용법 | 1 |
| `native/src/server/mod.rs` | `remote_router`, `trusted`, `stop_host`, `host_allowed`, 경로 합치기, 원격 패인 relay 제거, link/retry 경로 | 1, 3, 4, 5 |
| `native/src/server/host_client.rs` | `HostAction`, `keeping_running`, `http1` 사용 | 1, 3 |
| `native/src/server/http1.rs` (새) | 스트림 위의 HTTP/1.1 요청 하나 | 3 |
| `native/src/server/remote_window.rs` (새) | `/remote/<대상>/…` 경로 | 3, 5 |
| `native/src/server/remote/manager.rs` | 서버 대시보드 경로 사용 (2), 연결 유지 드라이버 (5) | 2, 5 |
| `native/src/protocol.rs` | `Remote` 종류 제거 (4), `Hello` 메시지 (5) | 4, 5 |
| `native/src/spawn.rs` | `Remote` 제거 | 4 |
| `native/tests/common/mod.rs` | `Viewer::attach_path` | 1 |
| `native/tests/remote.rs` | 서버 대시보드, gate, 원격 창 경로, 연결 유지 테스트 | 1–5 |
| `shared/protocol.ts` | `remote` 종류 제거 (4), link 메시지와 경로 (5) | 4, 5 |
| `shared/workspace-file.ts` | `legacyRemoteIds` (4), `isSshTarget` 공개 (6) | 4, 6 |
| `src/workspace.ts` | 원격 패인 필드 제거, `hydrate`가 옛 원격 패인을 닫음 (4), `keepTrying` (6) | 4, 6 |
| `src/remoteWindow.ts` (새) | 원격 창 대상, 서버 경로, 창 열기 | 6, 7 |
| `src/useRemoteLink.ts` (새) | 상태 흐름 | 6 |
| `src/components/RemoteConnect.tsx` (새) | 연결 화면, "재연결 중" 띠 | 6 |
| `src/useWorkspacePersistence.ts` | 서버 경로, 첫 읽기 규칙, 다시 연결 뒤 저장 | 6 |
| `src/useSshHosts.ts` | 서버 경로 | 6 |
| `src/terminal/connection.ts` | 원격 패인 메시지 제거 (4), 서버 경로와 닫기 알림 (6) | 4, 6 |
| `src/remoteApi.ts` | `shouldPostRemoteClose(target, …)` | 4, 6 |
| `src/components/Pane.tsx` | 원격 패인 표시 제거 (4), `keepTrying` (6) | 4, 6 |
| `src/kinds.tsx`, `src/components/ActivityBar.tsx` | `remote` 실행기 제거 (4), 원격 창 버튼 (7) | 4, 7 |
| `src/paletteCommands.tsx` | 원격 패인 명령 제거 (4), 원격 창 모드 명령 (6), "새 창에서 열기" (7) | 4, 6, 7 |
| `src/App.tsx` | 원격 패인 제거 (4), 원격 창 모드 (6), 진입점 (7) | 4, 6, 7 |
| `src/i18n/messages.ts` | 문구 | 4, 6, 7 |
| `tests/remote.test.ts` | UI 테스트 | 4, 6, 7 |
| `vite.config.ts` | `/remote` 프록시 (WebSocket 포함) | 3 |
| `macos/app/main.swift` | 앱 창 여러 개 | 8 |
| `docs/architecture.md`, `docs/sessions.md`, `docs/usage.md`, `CHANGELOG.md` | 문서 | 9 |

---

### Task 1: 서버 대시보드 (`westerm remote-dashboard`)

**Files:**
- Create: `native/src/remote/daemon.rs`, `native/src/remote/dashboard.rs`
- Modify: `native/src/remote/mod.rs` (`#[cfg(unix)] pub mod daemon; pub mod dashboard;`), `native/src/remote/gate.rs` (잠금·기록·멈춤과 `termination()`을 `daemon.rs`로 옮김), `native/src/ipc.rs`, `native/src/server/mod.rs`, `native/src/server/host_client.rs`, `native/src/main.rs`, `native/tests/common/mod.rs`
- Test: `native/src/server/host_client.rs` (unit), `native/tests/remote.rs`

**Interfaces:**
- Produces:
  - `remote::daemon::Daemon`: `const fn new(name: &'static str)`; `record_path(&self, state: &Path) -> PathBuf` (`<name>.json`); `lock(&self, state: &Path) -> io::Result<File>` (이미 잡혀 있으면 `ErrorKind::AddrInUse`); `is_running(&self, state) -> bool`; `read_record<T: DeserializeOwned>(&self, state) -> Option<T>`; `write_record<T: Serialize>(&self, state, record: &T) -> io::Result<()>` (임시 이름 → rename, 0600); `remove_record(&self, state)`; `async stop_running(&self, state) -> io::Result<bool>` (기록의 `pid`에 SIGTERM, 최대 5초 기다림, 안 돌면 false).
  - `remote::daemon::termination() -> &'static str` (async, SIGTERM/SIGINT/SIGHUP).
  - `remote::dashboard`: `DASHBOARD: Daemon` (`"dashboard"`), `socket_path(state: &Path) -> PathBuf` (`dashboard.sock`), `DashboardRecord { pid: u32, version: String }`, `read_record`, `is_running`, `async stop_running`, `async run(state: &Path) -> io::Result<()>`.
  - `server::remote_router(state_dir: PathBuf) -> axum::Router` (`#[cfg(unix)]`): `GET`/`PUT /api/workspace`, `GET /api/ssh-hosts`, `GET /ws/session`. 나머지는 404. 출처 확인 없음. host는 `HostClient::keeping_running()`.
  - `ipc::bind_socket(path: &Path) -> io::Result<tokio::net::UnixListener>` (unix): 임시 이름으로 bind, 0600, rename. `ipc::bind(dir)`가 이것을 쓴다.
  - `host_client::HostAction { Use, Replace, Start }`, `host_action(running: Option<&str>, local: &str, replace_outdated: bool) -> HostAction`, `HostClient::keeping_running() -> HostClient`. `HostClient::default()`는 지금처럼 교체한다.
  - 테스트 도우미 `common::Viewer::attach_path(stream: S, path_and_query: &str)`. `attach_over(stream, query)`는 `attach_path(stream, &format!("/session?{query}"))`가 된다.

- [ ] **Step 1: 실패하는 테스트 쓰기**

```rust
// native/src/server/host_client.rs, mod tests
#[test]
fn a_remote_dashboard_never_replaces_a_running_host() {
    assert_eq!(host_action(Some("rs-old"), "rs-new", false), HostAction::Use);
    assert_eq!(host_action(Some("rs-old"), "rs-new", true), HostAction::Replace);
    assert_eq!(host_action(Some("rs-new"), "rs-new", true), HostAction::Use);
    assert_eq!(host_action(None, "rs-new", false), HostAction::Start);
}
```

```rust
// native/tests/remote.rs
#[tokio::test]
async fn the_remote_dashboard_answers_only_its_routes_on_a_private_socket() {
    let state = state();
    let _host = StopHostOnDrop(state.path());
    let dashboard = Spawned(
        Command::new(BIN).arg("remote-dashboard").env("WESTERM_STATE_DIR", state.path()).env("SHELL", "/bin/sh").spawn().unwrap(),
    );
    let socket = wait_for_file(&state.path().join("dashboard.sock")).await;
    assert_eq!(std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(listening_tcp_ports(dashboard.0.id()).is_empty(), "no TCP port on the server");
    assert_eq!(unix_http(&socket, "GET", "/api/workspace", None).await, (200, "null".to_owned()));
    let saved = r#"{"sessions":[],"tabs":[],"version":1}"#; // write_workspace stores keys sorted
    assert_eq!(unix_http(&socket, "PUT", "/api/workspace", Some(saved)).await.0, 204, "no Origin needed on the socket");
    assert_eq!(unix_http(&socket, "GET", "/api/workspace", None).await.1, saved);
    for path in ["/", "/index.html", "/api/update", "/api/terminal-config", "/api/remote/decision"] {
        assert_eq!(unix_http(&socket, "GET", path, None).await.0, 404, "{path}");
    }
    let stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
    let (mut viewer, _) = Viewer::attach_path(stream, &format!("/ws/session?{QUERY}")).await;
    viewer.send(MARK_INPUT).await;
    viewer.wait_for("MARK_42").await;
    viewer.detach().await;
    let stop = Command::new(BIN).arg("stop").env("WESTERM_STATE_DIR", state.path()).output().unwrap();
    assert!(String::from_utf8_lossy(&stop.stdout).contains("the remote dashboard stopped"));
    assert!(!socket.exists());
}
```

새 테스트 도우미 (`native/tests/remote.rs`): `wait_for_file(path) -> PathBuf` (10초까지 기다림), `unix_http(socket, method, path, body: Option<&str>) -> (u16, String)` (요청 하나, `Connection: close`, 응답을 끝까지 읽고 상태와 본문을 나눔), `listening_tcp_ports(pid) -> Vec<String>` (`lsof -nP -a -p <pid> -iTCP -sTCP:LISTEN -Fn`; `lsof`가 없으면 빈 목록과 `eprintln!`).

- [ ] **Step 2: 실패 확인**

Run: `cargo test --manifest-path native/Cargo.toml -- a_remote_dashboard_never_replaces_a_running_host the_remote_dashboard_answers_only_its_routes_on_a_private_socket`
Expected: FAIL — `cannot find function host_action` (컴파일 오류).

- [ ] **Step 3: 구현**

- `daemon.rs`: gate의 `record_path`, `lock_path`, `is_running`, `stop_running`, `write_record`, `termination`을 옮겨 일반화한다. `gate::{read_record, is_running, stop_running}`은 같은 시그니처로 남기고 `Daemon::new("gate")`를 쓴다.
- `dashboard::run`: `DASHBOARD.lock(state)?` → `ipc::bind_socket(&socket_path(state))?` → `write_record(DashboardRecord { pid, version: crate::version_line() })` → `axum::serve(listener, server::remote_router(state.to_path_buf())).with_graceful_shutdown(termination())`. 끝날 때 기록을 지우고, 소켓을 지우고, 잠금을 놓는다 (gate와 같은 순서).
- `AppState`에 `trusted: bool`. `origin_allowed`는 `state.trusted`면 true. `remote_router`는 `trusted: true`, `HostClient::keeping_running()`으로 만든다. `.fallback(|| async { StatusCode::NOT_FOUND })`.
- `trusted`일 때 `session()`은 `SessionKind::Remote`를 403으로 거부한다 (Task 4에서 종류 자체가 사라진다).
- `HostClient`에 `replace_outdated: bool`. `start()`는 `host_action`으로 정한다.
- `stop_host()`: gate 다음에 `remote::dashboard::stop_running`이 true면 `westerm: the remote dashboard stopped`를 출력한다 (`#[cfg(unix)]`).
- `main.rs`: `Some(... | "remote-dashboard")`, unix는 `runtime.block_on(remote::dashboard::run(&state_dir()?))`, Windows는 `unix_only("remote-dashboard")`. `USAGE`에 `westerm remote-dashboard`를 더한다.

- [ ] **Step 4: 통과 확인**

Run: Step 2의 명령, 그다음 `cargo test --manifest-path native/Cargo.toml`
Expected: 두 테스트 PASS. 전체 PASS (기존 gate 테스트와 원격 패인 테스트 포함).

- [ ] **Step 5: 체크포인트** — 제안: `git add native && git commit -m "Add westerm remote-dashboard, a socket-only dashboard for remote windows"`

---

### Task 2: remote-setup이 서버 대시보드를 띄우고, gate가 그 소켓에 잇는다

**Files:**
- Modify: `native/src/remote/setup.rs`, `native/src/remote/report.rs`, `native/src/remote/gate.rs`, `native/src/server/remote/manager.rs`, `native/src/server/mod.rs`
- Test: `native/src/remote/report.rs` (unit), `native/tests/remote.rs`

**Interfaces:**
- Consumes: Task 1의 `dashboard::{socket_path, read_record, is_running, stop_running, DashboardRecord}`.
- Produces:
  - `SetupReport { v, version, port, fingerprint, dashboard: String, host: HostInfo, kill_on_logout }`, `SETUP_VERSION = 2`. `dashboard`는 서버 대시보드 소켓의 절대 경로. `socket` 필드는 없앤다.
  - gate: 받은 스트림마다 `dashboard.sock`에 연결하고 `tokio::io::copy_bidirectional`로 잇는다. 요청 검사는 없다.
  - 로컬은 서버 대시보드 경로로 요청한다: 원격 패인 relay와 `end_session`은 `ws://remote/ws/session?…`.

- [ ] **Step 1: 테스트 고치기와 쓰기**

- `report.rs`: `sample()`을 `v: 2`, `dashboard: "/s/dashboard.sock"`로 바꾼다. 새 테스트 `a_report_without_the_dashboard_is_refused`: v1 줄(`"v":1`, `"socket":…`)이면 `parse_report`가 `None`.
- `native/tests/remote.rs`:
  - `setup_starts_a_gate_that_relays_a_shell` → `setup_starts_a_dashboard_behind_the_gate`: `report["v"] == 2`; `report["dashboard"] == state/dashboard.sock`; QUIC 스트림에 `GET /api/workspace HTTP/1.1\r\nHost: remote\r\nConnection: close\r\n\r\n`을 보내면 응답이 `HTTP/1.1 200`으로 시작하고 `null`로 끝남; `Viewer::attach_path(stream(&connection).await, &format!("/ws/session?{QUERY}"))`로 셸에 붙어서 `MARK_42`.
  - `setup_twice_keeps_one_gate_and_one_authorized_line`: 두 번째 setup 뒤에도 `dashboard.json`의 `pid`가 같다.
  - `the_gate_refuses_unknown_clients_and_other_requests` → `the_gate_refuses_unknown_clients`: 모르는 인증서는 핸드셰이크 실패 (그대로). "다른 요청" 부분은 `GET /api/update` → `HTTP/1.1 404`로 바꾼다 (서버 대시보드가 거부).
  - `stop_ends_the_host_and_the_gate` → `stop_ends_the_host_the_gate_and_the_dashboard`: 출력에 `the remote dashboard stopped`, `dashboard.sock`이 없음.
- `gate.rs` unit: `only_websocket_session_upgrades_pass`, `an_accepted_upgrade_flows_both_ways`, `a_refused_upgrade_lets_nothing_else_through`, `other_requests_never_reach_the_host`를 지운다.

- [ ] **Step 2: 실패 확인**

Run: `cargo test --manifest-path native/Cargo.toml -- a_report_without_the_dashboard_is_refused setup_starts_a_dashboard_behind_the_gate setup_twice_keeps_one_gate the_gate_refuses_unknown_clients stop_ends_the_host_the_gate_and_the_dashboard`
Expected: FAIL — 보고서에 `dashboard`가 없음.

- [ ] **Step 3: 구현**

- `setup.rs`: `ensure_dashboard(state) -> io::Result<DashboardRecord>`. `ensure_gate`와 같은 규칙: 돌고 있고 `version`이 `version_line()`과 같으면 그대로; 아니면 멈추고 `westerm remote-dashboard`를 `spawn_detached`로 시작 (`dashboard.log`); 그 자식의 `pid`가 담긴 기록과 소켓 파일이 생길 때까지 최대 5초 기다림. `run()` 순서: host(규칙 그대로) → 서버 대시보드 → gate → 보고.
- `report.rs`: 필드와 버전. `parse_report`는 `v == SETUP_VERSION`인 줄만 받는다.
- `gate.rs`: `accept()`의 스트림 처리를 `UnixStream::connect(dashboard::socket_path(&state))` + `copy_bidirectional`로 바꾼다. `proxy`, `read_head`, `request_allowed`, `HEAD_LIMIT`, `HEAD_TIMEOUT`을 지운다. 모듈 설명을 고친다.
- `manager.rs`: `setup()`은 `report.dashboard`를 검사한다 (절대 경로, `:` 공백 줄바꿈 없음). `transport()`의 터널은 `&report.dashboard`로 연결한다. `end_session`의 URL은 `ws://remote/ws/session?session={id}&kind=shell&cols=80&rows=24&width=640&height=408&lang=en`.
- `server/mod.rs` `remote_relay`: `format!("ws://remote/ws/session?{}", request.for_remote_host().to_query())`.

- [ ] **Step 4: 통과 확인**

Run: Step 2의 명령, 그다음 `cargo test --manifest-path native/Cargo.toml`
Expected: PASS. v0.2.0 원격 패인 테스트도 PASS (서버 대시보드를 거쳐 같은 host에 붙는다).

- [ ] **Step 5: 체크포인트** — 제안: `git commit -m "Start the remote dashboard in remote-setup and pass gate streams to it"`

---

### Task 3: 로컬 대시보드의 원격 창 경로 (`/remote/<대상>/…`)

**Files:**
- Create: `native/src/server/http1.rs`, `native/src/server/remote_window.rs`
- Modify: `native/src/server/mod.rs` (경로 합치기, `host_allowed`), `native/src/server/host_client.rs` (`parse_response`를 `http1`로 옮기고 그것을 씀), `vite.config.ts` (`"/remote": { target: server, ws: true }`)
- Test: `native/src/server/http1.rs`, `native/src/server/mod.rs` (unit), `native/tests/remote.rs`

**Interfaces:**
- Consumes: `RemoteManager::{open_stream (지금은 필요하면 연결함), invalidate, remember_close}`, `pipe`, `REMOTE_HANDSHAKE_TIMEOUT`, `AttachRequest::{from_query, to_query}`, `normalize_ssh_target`.
- Produces:
  - 경로: `GET /remote/{target}/api/workspace`, `PUT /remote/{target}/api/workspace`, `GET /remote/{target}/api/ssh-hosts`, `GET /remote/{target}/ws/session` (WebSocket). 서버 대시보드의 같은 경로로 보낸다.
  - 응답: 대상이 틀리면 400. Host가 다르면 403. `PUT`의 출처나 JSON 형식이 틀리면 400 (`put_workspace`와 같음). WebSocket의 출처가 틀리면 403. 연결이 없으면 503. 서버가 답하지 않으면 502이고, 그 연결을 버린다 (`invalidate`).
  - `http1::exchange<S: AsyncRead + AsyncWrite + Unpin>(stream: S, method: &str, path: &str, body: Option<&[u8]>) -> Option<(u16, Vec<u8>)>`: 요청 하나 (`Host: westerm`, `Connection: close`, 본문이 있으면 `Content-Type: application/json`과 `Content-Length`), 10초 안에 끝까지 읽음.
  - `fn host_allowed(state: &AppState, headers: &HeaderMap) -> bool`: Host가 허용된 출처의 authority(`127.0.0.1:<포트>`, `localhost:<포트>`, `--allow-origin` 각각)와 같을 때만 true.

- [ ] **Step 1: 실패하는 테스트 쓰기**

```rust
// native/src/server/mod.rs, mod tests
#[test]
fn host_allowed_follows_the_allowed_origins() {
    let state = test_state(&["http://127.0.0.1:4317", "http://localhost:4317", "http://localhost:5173"]);
    for host in ["127.0.0.1:4317", "localhost:4317", "localhost:5173"] {
        assert!(host_allowed(&state, &headers_with_host(host)), "{host}");
    }
    assert!(!host_allowed(&state, &headers_with_host("evil.example:4317")));
    assert!(!host_allowed(&state, &HeaderMap::new()));
}
```

```rust
// native/src/server/http1.rs, mod tests
#[tokio::test]
async fn exchange_sends_one_request_and_reads_to_the_end() {
    let (client, mut server) = tokio::io::duplex(4096);
    let answer = tokio::spawn(async move {
        let request = read_until(&mut server, b"[]").await; // 머리와 본문
        server.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").await.unwrap();
        String::from_utf8(request).unwrap()
    });
    assert_eq!(exchange(client, "PUT", "/api/workspace", Some(b"[]")).await, Some((204, Vec::new())));
    let request = answer.await.unwrap();
    assert!(request.starts_with("PUT /api/workspace HTTP/1.1\r\n"));
    assert!(request.contains("Content-Length: 2\r\n") && request.contains("Connection: close\r\n"));
}
// reads_plain_and_chunked_responses는 host_client.rs에서 이리로 옮긴다.
```

```rust
// native/tests/remote.rs (FakeRemote 사용)
#[tokio::test]
async fn a_remote_window_saves_and_reads_the_servers_layout() {
    let fake = FakeRemote::start(None);
    let saved = r#"{"sessions":[],"tabs":[],"version":1}"#; // write_workspace stores keys sorted
    assert_eq!(http(fake.port, "PUT", "/remote/fake/api/workspace", Some(saved), true).await.0, 204);
    assert_eq!(std::fs::read_to_string(fake.path().join("rstate/workspace.json")).unwrap(), saved);
    assert_eq!(http(fake.port, "GET", "/remote/fake/api/workspace", None, true).await, (200, saved.to_owned()));
    assert!(!fake.path().join("lstate/workspace.json").exists(), "the local layout is untouched");
}

#[tokio::test]
async fn a_remote_window_opens_a_shell_on_the_server() {
    let fake = FakeRemote::start(None);
    let (mut pane, ready) = window_pane(fake.port, "fake", "sw1").await;
    assert_eq!(ready["mode"], "new");
    pane.send(MARK_INPUT).await;
    pane.wait_for("MARK_42").await;
    pane.detach().await;
    let (pane, ready) = window_pane(fake.port, "fake", "sw1").await;
    assert_eq!(ready["mode"], "reattached");
    assert!(pane.output.contains("MARK_42"), "the screen comes back");
}

#[tokio::test]
async fn remote_window_routes_check_origin_and_host() {
    let fake = FakeRemote::start(None);
    let body = r#"{"version":1,"tabs":[],"sessions":[]}"#;
    assert_eq!(http(fake.port, "PUT", "/remote/fake/api/workspace", Some(body), false).await.0, 400, "no Origin");
    assert_eq!(http_with_host(fake.port, "GET", "/remote/fake/api/workspace", "evil.example:80").await.0, 403, "DNS rebinding");
    assert_eq!(http(fake.port, "GET", "/remote/-oProxyCommand=x/api/workspace", None, true).await.0, 400);
    let query = "session=sw2&kind=shell&cols=80&rows=24&width=640&height=408&lang=en";
    assert_eq!(ws_status(fake.port, &format!("/remote/fake/ws/session?{query}"), Some("http://evil.example")).await, 403);
    assert_eq!(fake.count(PROBE), 0, "refused requests never reach ssh");
}
```

새 테스트 도우미: `http(port, method, path, body, with_origin) -> (u16, String)` (`post`를 일반화), `http_with_host(port, method, path, host)`, `ws_status(port, path_and_query, origin: Option<&str>) -> u16` (거부된 업그레이드의 상태 코드), `window_pane(port, target, session) -> (Pane, ready)` (`/remote/<target>/ws/session?…&window=wa`, 올바른 Origin, 크기 먼저 보내고 `ready`까지 읽음).

- [ ] **Step 2: 실패 확인**

Run: `cargo test --manifest-path native/Cargo.toml -- host_allowed_follows_the_allowed_origins exchange_sends_one_request a_remote_window_ remote_window_routes_check_origin_and_host`
Expected: FAIL — `host_allowed`, `exchange`가 없음.

- [ ] **Step 3: 구현**

- `remote_window.rs`: `pub(super) fn routes() -> Router<Shared>`. 모든 처리기는 먼저 대상(`Path<String>` → `normalize_ssh_target`), 그다음 `host_allowed`를 본다.
  - `GET`과 `PUT` workspace, `GET` ssh-hosts: `state.remote.open_stream(&target)` → `http1::exchange(stream, method, path, body)` → 같은 상태 코드와 본문으로 답한다. 200 본문은 `application/json`, `no-store`.
  - WebSocket: 출처 확인, `AttachRequest::from_query`, `SessionKind::Remote`는 403 (Task 4까지). 업그레이드 전에 `open_stream`을 기다린다 (실패 → 503). 업그레이드 뒤 `ws://remote/ws/session?{request.to_query()}`로 `REMOTE_HANDSHAKE_TIMEOUT` 안에 핸드셰이크 (실패 → `invalidate`, 1011로 닫음), 그다음 `pipe`. `session_closed && !host_answered`면 `remember_close`와 `invalidate`.
- `server/mod.rs`: `.merge(remote_window::routes())`. `host_allowed`는 `state.origins`에서 `http://`를 뗀 값과 Host를 비교한다.
- `host_client.rs`: `request()`는 `http1::exchange`를 쓴다.
- `vite.config.ts`: `/remote` 프록시.

- [ ] **Step 4: 통과 확인**

Run: Step 2의 명령, `cargo test --manifest-path native/Cargo.toml`
Expected: PASS.

- [ ] **Step 5: 체크포인트** — 제안: `git add native vite.config.ts && git commit -m "Pass remote window requests from the local dashboard to the server"`

---

### Task 4: 원격 패인 없애기 (Rust + TS), v0.2.0 레이아웃 옮기기

**Files:**
- Modify (Rust): `native/src/protocol.rs` (`SessionKind::Remote`, `for_remote_host`, `CLOSE_REMOTE_FAILED` 제거; `is_text`; 대상이 필요한 종류는 `Ssh`만), `native/src/spawn.rs` (`Remote` 갈래), `native/src/server/mod.rs` (`remote_relay`, `stage_message`, `Remote` 갈래들, `remote_router`와 `remote_window`의 임시 거부), `native/tests/remote.rs` (원격 패인 테스트 5개와 `connect_pane`, `open_pane` 제거: `a_remote_pane_installs_westerm_and_survives_a_reconnect`, `two_panes_opening_at_once_install_once`, `closing_a_pane_while_connecting_ends_it_on_the_server`, `a_lost_gate_comes_back_and_the_shell_is_reattached`, `closing_a_pane_over_a_silent_link_ends_it_later`. Task 5가 원격 창 버전을 만든다.)
- Modify (TS): `shared/protocol.ts` (`SessionKind`, `isSessionKind`, `CLOSE_REMOTE_FAILED`), `shared/workspace-file.ts`, `src/workspace.ts`, `src/kinds.tsx`, `src/components/ActivityBar.tsx`, `src/App.tsx`, `src/components/Pane.tsx`, `src/terminal/connection.ts` (`remote`/`remoteFailed` 이벤트와 `remote` 메시지 갈래, 닫기 알림), `src/remoteApi.ts` (`shouldPostRemoteClose` 제거, `postRemote`는 남김), `src/paletteCommands.tsx` (`remote-<대상>`, `remote-update-<대상>` 제거), `src/i18n/messages.ts` (`kinds.remote`, `palette.remoteWesterm`, `palette.remoteWestermHint`, `palette.updateRemoteHost` 제거; `remote.*` 단계·실패 문구는 남김)
- Test: `tests/remote.test.ts` (다시 씀), `native/src/protocol.rs` (unit)

**Interfaces:**
- Produces:
  - `SavedDashboard.legacyRemoteIds: string[]`: v0.2.0 원격 패인의 id. 파서는 이 세션을 같은 `target`의 `ssh` 자리표시 세션으로 읽어서 나머지 검증(탭, 트리, 워크스페이스)을 그대로 통과시킨다. 대상이 틀린 원격 세션은 지금처럼 파일 전체를 거부한다.
  - `dashboardReducer`의 `hydrate`는 `legacyRemoteIds`를 `close`와 같은 규칙으로 닫는다. 비게 된 탭은 남기지 않는다.
  - `SessionKind`(Rust, TS)에 `remote`가 없다. `RemoteStage`, `RemoteFailure`, `RemoteVia`, `ServerMessage::Remote`, `REMOTE_*_PATH`, `/api/remote/{decision,close,update-host}`는 남긴다 (Task 5, 6이 쓴다).
  - `disconnected`의 원격 예외(`session.kind !== "remote"`)는 없앤다. Task 6이 `keepTrying`으로 다시 넣는다.

- [ ] **Step 1: 실패하는 테스트 쓰기**

```ts
// tests/remote.test.ts
test("v0.2.0 remote panes leave the saved layout; the rest stays", () => {
  const saved = parseSavedDashboard({
    version: 1,
    activeTabId: "t1",
    tabs: [
      { id: "t1", name: null, paneIds: ["sa", "sr"], focusedId: "sr", maximizedId: null },
      { id: "t2", name: null, paneIds: ["sq"], focusedId: "sq", maximizedId: null },
    ],
    sessions: [
      { id: "sa", kind: "shell", title: "zsh", name: null, url: null },
      { id: "sr", kind: "remote", target: "prod", title: "westerm prod", name: null, url: null },
      { id: "sq", kind: "remote", target: "prod", title: "westerm prod", name: null, url: null },
    ],
  });
  assert.ok(saved);
  assert.deepEqual(saved.legacyRemoteIds, ["sr", "sq"]);
  const state = dashboardReducer(createDashboard("w1", "t1"), { type: "hydrate", saved });
  assert.deepEqual(state.sessions.map((session) => session.id), ["sa"]);
  assert.equal(state.tabs.length, 1, "a tab left empty goes");
  assert.deepEqual(state.tabs[0].paneIds, ["sa"]);
  assert.equal(activeTab(state).focusedId, "sa");
});

test("a saved remote pane still needs a valid destination", () => {
  assert.equal(layout({ kind: "remote", target: "-oProxyCommand=x" }), null);
  assert.equal(layout({ kind: "remote" }), null);
});

test("remote is no longer a session kind", () => {
  assert.equal(isSessionKind("remote"), false);
});

test("the ssh picker offers no westerm panes", () => {
  const commands = sshCommands([{ alias: "prod", hostname: null, user: null }], null, () => {}, MESSAGES.en);
  assert.equal(commands.some((command) => command.id.startsWith("remote-")), false);
  assert.ok(commands.some((command) => command.id === "ssh-prod"));
});
// "only some remote failures are permanent"(isPermanentFailure)는 그대로 둔다.
```

```rust
// native/src/protocol.rs, mod tests
#[test]
fn remote_is_no_longer_a_session_kind() {
    assert_eq!(SessionKind::parse("remote"), None);
    let query: HashMap<String, String> =
        [("session", "sr1"), ("kind", "remote"), ("target", "prod"), ("cols", "80"), ("rows", "24"), ("width", "640"), ("height", "408")]
            .into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect();
    assert_eq!(AttachRequest::from_query(&query), None);
}
```

- [ ] **Step 2: 실패 확인**

Run: `node --import tsx --test tests/remote.test.ts` 그리고 `cargo test --manifest-path native/Cargo.toml -- remote_is_no_longer_a_session_kind`
Expected: FAIL — `legacyRemoteIds`가 없음, `isSessionKind("remote")`가 true, `SessionKind::parse("remote")`가 `Some`.

- [ ] **Step 3: 구현** — 위 Files 목록대로 지운다. `hydrate`는 기존 `close` 처리 함수를 id마다 적용한다 (세션을 끝내는 효과 없음: 패인이 아직 붙지 않았다). `Pane.tsx`의 `remoteDetail`과 확인 버튼은 지운다 (Task 6이 연결 화면에 새로 만든다). 실행기 목록(`LAUNCHERS`)에서 `remote`를 빼고, 서버 아이콘은 Task 7을 위해 `RemoteWindowIcon`으로 남긴다.

- [ ] **Step 4: 통과 확인**

Run: `npm run type-check`, `node --import tsx --test tests/*.test.ts`, `cargo test --manifest-path native/Cargo.toml`
Expected: 모두 PASS. `RemoteManager::stages`처럼 쓰는 곳이 잠시 없어진 Rust 함수의 dead_code 경고는 Task 5에서 사라진다.

- [ ] **Step 5: 체크포인트** — 제안: `git add -A native shared src tests && git commit -m "Remove the remote westerm pane and drop saved remote panes from layouts"`

---

### Task 5: 연결 유지와 상태 흐름 (`/api/remote/link`), 다시 연결 규칙

**Files:**
- Modify: `native/src/server/remote/manager.rs`, `native/src/server/mod.rs` (`AppState.remote: Arc<RemoteManager>`, 경로 `/api/remote/link`, `/api/remote/retry`), `native/src/server/remote_window.rs` (`open_stream`이 `None`이면 503), `native/src/protocol.rs` + `shared/protocol.ts` (`ServerMessage::Hello { version }`, `REMOTE_LINK_PATH`, `REMOTE_RETRY_PATH`)
- Test: `native/src/server/remote/manager.rs` (unit), `native/tests/remote.rs`

**Interfaces:**
- Consumes: Task 3 경로, Task 2의 서버 대시보드 경로.
- Produces:
  - `RemoteManager::watch(self: &Arc<Self>, name: &str) -> Watch`; `pub struct Watch { pub stages: watch::Receiver<Option<RemoteStage>>, /* drop하면 지켜보는 수가 줄어듦 */ }`. `Watch`가 하나라도 있으면 그 대상의 드라이버 태스크가 연결을 유지한다. 마지막 `Watch`가 사라지면 `linger()` 뒤에 연결을 닫는다.
  - `RemoteManager::retry(&self, name: &str) -> bool`: 영구 실패로 멈춘 드라이버를 깨운다. 기다리는 드라이버가 없으면 false.
  - `RemoteManager::open_stream(&self, name: &str) -> Option<Opened>`: 살아 있는 연결 위의 스트림만. 스스로 연결하지 않는다.
  - `pub fn retry_delay(failures: u32) -> Duration`: 1, 2, 4, 8초, 그 뒤 8초.
  - `pub fn ssh_due(last_failed_ssh: Option<Instant>, now: Instant) -> bool`: SSH 준비가 실패한 뒤 30초(`SSH_RETRY_INTERVAL`) 안에는 false.
  - `fn linger() -> Duration`: 30초. `WESTERM_REMOTE_LINGER_MS`가 있으면 그 값 (테스트용).
  - 상태 흐름 `GET /api/remote/link?target=<대상>` (WebSocket, 출처와 Host 확인): 처음에 `{"type":"hello","version":"<version_line()>"}`, 그다음 단계마다 `{"type":"remote","stage":…}` (지금 단계를 바로 한 번).
  - `POST /api/remote/retry {"target"}` → 204, 기다리는 것이 없으면 409. JSON과 출처 확인.
  - 드라이버 규칙: 일시 실패(`!is_permanent()`)는 `Failed`를 알리고 `retry_delay(n)` 뒤 다시 시도한다 (빠른 길은 매번, SSH 준비는 `ssh_due`일 때만). 영구 실패는 `Failed`를 알리고 `retry` 또는 지켜보는 이가 없을 때까지 기다린다. 살아 있던 연결이 죽으면(`is_alive` false, 1초마다 확인) `Connecting`을 알리고 바로 다시 붙는다 (빠른 길, 그다음 SSH). 새 연결마다 `deliver_closes`를 실행한다.
  - `update_host`는 그대로: 교체 뒤 드라이버가 새 연결을 만든다.

- [ ] **Step 1: 실패하는 테스트 쓰기**

```rust
// native/src/server/remote/manager.rs, mod tests
#[test]
fn retry_delays_grow_to_eight_seconds() {
    let secs: Vec<u64> = (0..6).map(|n| retry_delay(n).as_secs()).collect();
    assert_eq!(secs, [1, 2, 4, 8, 8, 8]);
}

#[test]
fn ssh_waits_thirty_seconds_after_a_failed_setup() {
    let now = Instant::now();
    assert!(ssh_due(None, now));
    assert!(!ssh_due(Some(now - Duration::from_secs(29)), now));
    assert!(ssh_due(Some(now - Duration::from_secs(30)), now));
}
```

```rust
// native/tests/remote.rs
#[tokio::test]
async fn a_watched_link_shows_its_stages_until_connected() {
    let fake = FakeRemote::start(None);
    let (mut link, hello) = open_link(fake.port, "fake").await;
    assert_eq!(hello["type"], "hello");
    assert!(hello["version"].as_str().unwrap().starts_with("westerm "));
    assert_eq!(stages_until_connected(&mut link).await, ["checking", "installing", "starting", "connecting", "connected"]);
}

#[tokio::test]
async fn two_windows_opening_at_once_set_up_once() {
    let fake = FakeRemote::start(Some("1"));
    let (mut a, _) = open_link(fake.port, "fake").await;
    let (mut b, _) = open_link(fake.port, "fake").await;
    stages_until_connected(&mut a).await;
    stages_until_connected(&mut b).await;
    assert_eq!(fake.count(UPLOAD), 1);
    assert_eq!(fake.count(SETUP), 1);
}

#[tokio::test]
async fn remote_window_requests_wait_for_a_watched_link() {
    let fake = FakeRemote::start(None);
    assert_eq!(http(fake.port, "GET", "/remote/fake/api/workspace", None, true).await.0, 503, "no window watches the link yet");
    assert_eq!(fake.count(PROBE), 0, "requests alone never start ssh");
    let (mut link, _) = open_link(fake.port, "fake").await;
    stages_until_connected(&mut link).await;
    assert_eq!(http(fake.port, "GET", "/remote/fake/api/workspace", None, true).await.0, 200);
}

#[tokio::test]
async fn a_permanent_failure_waits_for_retry() {
    let fake = FakeRemote::start(None);
    let newer = fake.path().join("home/.local/bin/westerm");
    std::fs::create_dir_all(newer.parent().unwrap()).unwrap();
    write_script(&newer, "#!/bin/sh\necho 'westerm 99.0.0 (host rs-0)'\n");
    let (mut link, _) = open_link(fake.port, "fake").await;
    let failed = stage_named(&mut link, "failed").await;
    assert_eq!(failed["reason"], "newerRemote");
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(fake.count(PROBE), 1, "no retry on its own");
    std::fs::remove_file(&newer).unwrap();
    assert_eq!(post(fake.port, "/api/remote/retry", true, r#"{"target":"fake"}"#).await, 204);
    stages_until_connected(&mut link).await;
}

#[tokio::test]
async fn the_link_closes_after_the_last_window_leaves() {
    let fake = FakeRemote::start_with(None, &[("WESTERM_REMOTE_LINGER_MS", "200")]);
    let (mut link, _) = open_link(fake.port, "fake").await;
    stages_until_connected(&mut link).await;
    drop(link);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(http(fake.port, "GET", "/remote/fake/api/workspace", None, true).await.0, 503);
}
```

v0.2.0 테스트를 원격 창 경로로 옮긴다 (같은 방법으로 gate를 멈추거나 연결을 조용하게 만든다):

- `a_lost_gate_comes_back_and_the_shell_is_reattached`: 연결 → `window_pane(…, "sw1")`에서 `MARK_42` → gate를 멈춤 → 패인 소켓이 끝남 → 상태 흐름이 다시 `connected` → `window_pane(…, "sw1")`의 `mode`가 `reattached`이고 출력에 `MARK_42`.
- `a_session_closed_while_disconnected_ends_after_reconnect`: 연결 → `sw2` 셸 → 연결을 조용하게 만듦 → `POST /api/remote/close {"target":"fake","session":"sw2"}` → 204 → 연결이 돌아온 뒤 `window_pane(…, "sw2")`의 `mode`가 `new`.
- `remote_apis_check_the_origin`: 기존 검사에 더한다. `POST /api/remote/retry` 출처 없음 → 400, `ws_status(port, "/api/remote/link?target=fake", None)` → 403.
- Task 3의 `a_remote_window_saves_and_reads_the_servers_layout`, `a_remote_window_opens_a_shell_on_the_server`는 먼저 `open_link` + `stages_until_connected`를 한다.

새 테스트 도우미: `open_link(port, target) -> (Socket, hello)`, `stages_until_connected(&mut Socket) -> Vec<String>` (`stage` 이름들, `connected`까지, 30초 제한), `stage_named(&mut Socket, name) -> serde_json::Value`, `FakeRemote::start_with(ssh_delay, env: &[(&str, &str)])` (`start`는 이것을 부름).

- [ ] **Step 2: 실패 확인**

Run: `cargo test --manifest-path native/Cargo.toml -- retry_delays ssh_waits a_watched_link two_windows remote_window_requests_wait a_permanent_failure the_link_closes a_lost_gate a_session_closed_while_disconnected remote_apis_check_the_origin a_remote_window_`
Expected: FAIL — `/api/remote/link`가 없음 (404), `retry_delay`가 없음.

- [ ] **Step 3: 구현**

- `Target`에 지켜보는 수(`AtomicUsize`), 깨우기(`Notify`), 드라이버가 도는지(`AtomicBool`, 대상마다 하나), 영구 실패 대기와 `retry` 신호, 마지막 SSH 실패 시각(`Mutex<Option<Instant>>`).
- `connect()`를 `connect_fast(&self, name, target) -> Option<Link>`(저장된 기록, `!stale`, 버전 일치, `FAST_PATH_TIMEOUT`)와 `connect_full(&self, name, target) -> Result<Link, Failure>`(나머지 그대로)로 나눈다.
- `async fn drive(self: Arc<Self>, name: String, target: Arc<Target>)`: 위 규칙. 지켜보는 이가 0이 되면 `linger()`를 기다리고, 그래도 0이면 연결을 버리고 끝난다.
- 상태 흐름 처리기: 출처, Host, 대상 확인 → 업그레이드 → `watch()` → `hello` → `stages.changed()`마다 `ServerMessage::Remote(stage)` → 브라우저가 닫으면 끝 (`Watch` drop).
- `ServerMessage::Hello { version: String }` (Rust)와 `{ type: "hello"; version: string }` (TS), `REMOTE_LINK_PATH = "/api/remote/link"`, `REMOTE_RETRY_PATH = "/api/remote/retry"`.

- [ ] **Step 4: 통과 확인**

Run: Step 2의 명령, `cargo test --manifest-path native/Cargo.toml`, `npm run type-check`
Expected: PASS. dead_code 경고 없음.

- [ ] **Step 5: 체크포인트** — 제안: `git add native shared && git commit -m "Keep a server link up while a remote window watches it, and retry by the rules"`

---

### Task 6: 원격 창 모드 화면

**Files:**
- Create: `src/remoteWindow.ts`, `src/useRemoteLink.ts`, `src/components/RemoteConnect.tsx`
- Modify: `shared/workspace-file.ts` (대상 검사를 `isSshTarget`으로 공개), `src/useWorkspacePersistence.ts`, `src/useSshHosts.ts`, `src/terminal/connection.ts`, `src/remoteApi.ts`, `src/workspace.ts`, `src/components/Pane.tsx`, `src/App.tsx`, `src/paletteCommands.tsx`, `src/i18n/messages.ts`
- Test: `tests/remote.test.ts`

**Interfaces:**
- Consumes: Task 5의 상태 흐름 메시지(`hello`, `remote`)와 `REMOTE_LINK_PATH`, `REMOTE_RETRY_PATH`; 기존 `REMOTE_DECISION_PATH`, `REMOTE_CLOSE_PATH`, `REMOTE_UPDATE_HOST_PATH`, `postRemote`; Task 3 경로.
- Produces:
  - `remoteTargetFrom(search: string): string | null` (`isSshTarget`으로 검사); `REMOTE_TARGET: string | null` (`location`이 없으면 null); `serverPath(path: string, target: string | null = REMOTE_TARGET): string` — `WORKSPACE_PATH`, `SSH_HOSTS_PATH`, `WS_PATH`에만 `/remote/${encodeURIComponent(target)}`를 붙인다.
  - `INITIAL_LINK`, `linkReducer(state: LinkState, message: LinkMessage): LinkState`, `LinkState { stage: RemoteStage | null; connected: boolean; connectedOnce: boolean; version: string | null; reload: boolean }`, `useRemoteLink(target: string | null): LinkState` (상태 소켓이 닫히면 1초 뒤 다시 엶).
  - `firstLoad(remote: boolean, result: { ok: true; saved: SavedDashboard | null } | { ok: false }): "hydrate" | "fresh" | "retry"`.
  - `useWorkspacePersistence(state, dispatch, fresh, paused, link: { ready: boolean; reconnects: number })`.
  - `shouldPostRemoteClose(target: string | null, end: boolean, readyState: number): boolean`.
  - `disconnected` 액션: `{ type: "disconnected"; id: string; keepTrying: boolean }`.
  - `remoteWindowCommands(target: string, t: Messages): PaletteCommand[]` ("이 서버의 westerm host 업데이트").
  - 새 문구 (네 언어): `remote.connectingTo(target)`, `remote.reconnecting`, `remote.retry`, `palette.updateThisHost`.

- [ ] **Step 1: 실패하는 테스트 쓰기**

```ts
// tests/remote.test.ts
test("a remote window names its server in the address", () => {
  assert.equal(remoteTargetFrom("?remote=prod"), "prod");
  assert.equal(remoteTargetFrom("?remote=pista%40msi"), "pista@msi");
  assert.equal(remoteTargetFrom("?remote=-oProxyCommand%3Dx"), null);
  assert.equal(remoteTargetFrom(""), null);
});

test("a remote window sends layout, host list, and sessions to its server", () => {
  assert.equal(serverPath(WORKSPACE_PATH, "prod"), "/remote/prod/api/workspace");
  assert.equal(serverPath(SSH_HOSTS_PATH, "pista@msi"), "/remote/pista%40msi/api/ssh-hosts");
  assert.equal(serverPath(WS_PATH, "prod"), "/remote/prod/ws/session");
  assert.equal(serverPath(CONFIG_PATH, "prod"), CONFIG_PATH, "fonts stay local");
  assert.equal(serverPath(WORKSPACE_PATH, null), WORKSPACE_PATH);
});

test("the first load never replaces a server layout it could not read", () => {
  assert.equal(firstLoad(true, { ok: false }), "retry");
  assert.equal(firstLoad(false, { ok: false }), "fresh");
  assert.equal(firstLoad(true, { ok: true, saved: null }), "fresh");
  assert.equal(firstLoad(true, { ok: true, saved: SAMPLE_SAVED }), "hydrate");
});

test("the link state follows the stream", () => {
  let link = linkReducer(INITIAL_LINK, { type: "hello", version: "westerm 0.3.0 (host rs-a)" });
  link = linkReducer(link, { type: "remote", stage: "checking" });
  assert.equal(link.connected, false);
  link = linkReducer(link, { type: "remote", stage: "connected", via: "quic", killOnLogout: false });
  assert.ok(link.connected && link.connectedOnce);
  link = linkReducer(link, { type: "remote", stage: "failed", reason: "unreachable", detail: "" });
  assert.ok(!link.connected && link.connectedOnce && !link.reload);
  link = linkReducer(link, { type: "hello", version: "westerm 0.3.1 (host rs-b)" });
  assert.equal(link.reload, true, "a new local westerm reloads the window");
});

test("remote window panes keep reconnecting; local shells still give up", () => {
  let remote = dashboardReducer(createDashboard("w1", "t1"), { type: "open", id: "ss1", kind: "shell" });
  for (let i = 0; i < 20; i++) remote = dashboardReducer(remote, { type: "disconnected", id: "ss1", keepTrying: true });
  assert.equal(remote.sessions[0].notice, "reconnecting");
  let local = dashboardReducer(createDashboard("w1", "t1"), { type: "open", id: "ss1", kind: "shell" });
  for (let i = 0; i <= MAX_RECONNECTS; i++) local = dashboardReducer(local, { type: "disconnected", id: "ss1", keepTrying: false });
  assert.equal(local.sessions[0].status, "error");
});

test("a pane closed while its socket is down reports the close", () => {
  assert.equal(shouldPostRemoteClose("prod", true, 3), true, "closed socket");
  assert.equal(shouldPostRemoteClose("prod", true, 2), true, "closing socket");
  assert.equal(shouldPostRemoteClose("prod", true, 1), false, "an open socket carries it");
  assert.equal(shouldPostRemoteClose("prod", true, 0), false, "a connecting socket sends it once open");
  assert.equal(shouldPostRemoteClose("prod", false, 3), false, "only detaching");
  assert.equal(shouldPostRemoteClose(null, true, 3), false, "a local window");
});

// SAMPLE_SAVED: 이 파일 위쪽의 `layout({ kind: "shell" })` 결과처럼, 세션이 하나 있는 작은 저장 레이아웃.

test("a remote window's palette updates its server's host", () => {
  const [command] = remoteWindowCommands("prod", MESSAGES.en);
  assert.equal(command.title, MESSAGES.en.palette.updateThisHost);
  assert.match(command.keywords, /update/);
});
```

- [ ] **Step 2: 실패 확인**

Run: `node --import tsx --test tests/remote.test.ts`
Expected: FAIL — `../src/remoteWindow.ts` 모듈이 없음.

- [ ] **Step 3: 구현**

- `useWorkspacePersistence`: `serverPath(WORKSPACE_PATH)`. 첫 읽기는 `link.ready`가 true일 때 한다. 읽기는 실패와 "저장 없음(`null`)"을 나눠서 돌려주고, `firstLoad`가 "retry"면 다음 `ready` 때 또는 2초 뒤 다시 읽는다. `link.reconnects`가 늘면 마지막 상태를 한 번 저장한다.
- `useSshHosts`: `serverPath(SSH_HOSTS_PATH)`.
- `connection.ts`: `serverPath(WS_PATH)`. `close()`에서 `shouldPostRemoteClose(REMOTE_TARGET, end, this.ws.readyState)`면 `postRemote(REMOTE_CLOSE_PATH, { target: REMOTE_TARGET, session })`.
- `Pane.tsx`: `{ type: "disconnected", id, keepTrying: REMOTE_TARGET !== null }`.
- `App.tsx` (원격 창 모드일 때만):
  - `document.title = \`westerm — ${REMOTE_TARGET}\``.
  - 머리말에 서버 이름과 연결 방식(`t.remote.via[via]`: `QUIC` 또는 `SSH tunnel`)을 늘 보여 준다 (스펙 8.4).
  - `useRemoteLink(REMOTE_TARGET)`. `reload`면 `location.reload()`.
  - 처음 연결되기 전에는 `<RemoteConnect>`가 창 전체를 덮는다. 연결된 적이 있는데 지금 연결이 없으면 `<RemoteBanner>`를 보여 준다.
  - 영구 실패면 이유(`t.remote.failed[reason]`), detail, `t.remote.retry` 버튼(`REMOTE_RETRY_PATH`)을 보여 준다.
  - `confirm` 단계면 `t.remote.confirm(sessions)`와 두 버튼(`t.remote.replace`, keep 문구)을 보여 준다 (`REMOTE_DECISION_PATH`).
  - 업데이트 버튼, 팔레트의 업데이트 명령을 숨긴다. 팔레트에 `remoteWindowCommands(REMOTE_TARGET, t)`를 더한다.
  - persistence에 `{ ready: link.connected, reconnects }`를 넘긴다. `reconnects`는 처음 연결 뒤 연결이 false → true로 바뀐 횟수다. 로컬 창은 `{ ready: true, reconnects: 0 }`.
- `RemoteConnect.tsx`: 단계 문구 함수 `stageText(stage, t)` (checking, installing(version), starting, connecting, connected via, killOnLogout, failed 이유).

- [ ] **Step 4: 통과 확인**

Run: `npm run type-check`, `node --import tsx --test tests/*.test.ts`, `npx vite build`
Expected: PASS, 빌드 성공.

- [ ] **Step 5: 체크포인트** — 제안: `git add shared src tests && git commit -m "Run the dashboard page as a remote window for ?remote=<server>"`

---

### Task 7: 로컬 창에서 원격 창 열기

**Files:**
- Modify: `src/remoteWindow.ts` (`openRemoteWindow`), `src/paletteCommands.tsx`, `src/components/ActivityBar.tsx`, `src/kinds.tsx`, `src/App.tsx`, `src/i18n/messages.ts`
- Test: `tests/remote.test.ts`

**Interfaces:**
- Consumes: Task 6의 `REMOTE_TARGET` (원격 창에서는 진입점을 숨김), Task 4의 `RemoteWindowIcon`.
- Produces:
  - `openRemoteWindow(target: string, open: (url: string, name: string, features: string) => Window | null = window.open.bind(window)): void` — URL `/?remote=${encodeURIComponent(target)}`, 이름 `westerm-remote-${encodeURIComponent(target)}`, 옵션 `"popup,width=1280,height=820"`. 그 대상의 창이 아직 열려 있으면 다시 열지 않고 `focus()`한다.
  - `sshCommands(hosts, query, open, openWindow: ((target: string) => void) | null, t)`: 호스트마다(입력한 `user@host` 포함) `window-<대상>` 명령 "새 창에서 열기: <대상>". `openWindow`가 null이면 만들지 않는다.
  - 활동 막대: 로컬 창에서만 원격 창 버튼. 누르면 SSH 호스트 고르기를 "새 창" 모드로 연다.
  - 새 문구 (네 언어): `palette.openRemoteWindow(target)`, `palette.openRemoteWindowHint`, 활동 막대의 원격 창 이름과 설명, 팔레트 제목.

- [ ] **Step 1: 실패하는 테스트 쓰기**

```ts
test("the ssh picker opens each host in a new window", () => {
  const opened: string[] = [];
  const commands = sshCommands([{ alias: "prod", hostname: null, user: null }], null, () => {}, (target) => opened.push(target), MESSAGES.en);
  const entry = commands.find((command) => command.id === "window-prod");
  assert.ok(entry);
  assert.equal(entry.title, MESSAGES.en.palette.openRemoteWindow("prod"));
  entry.run();
  assert.deepEqual(opened, ["prod"]);
  assert.ok(commands.some((command) => command.id === "ssh-prod"), "plain ssh stays");
});

test("a typed user@host opens in a new window too", () => {
  const commands = sshCommands([], "pista@msi", () => {}, () => {}, MESSAGES.en);
  assert.ok(commands.some((command) => command.id === "window-pista@msi"));
});

test("a remote window offers no windows of its own", () => {
  const commands = sshCommands([{ alias: "prod", hostname: null, user: null }], null, () => {}, null, MESSAGES.en);
  assert.equal(commands.some((command) => command.id.startsWith("window-")), false);
});

test("a remote window opens once per server and is focused after", () => {
  const calls: unknown[][] = [];
  let focused = 0;
  const handle = { closed: false, focus: () => void focused++ } as unknown as Window;
  const open = (...args: [string, string, string]) => (calls.push(args), handle);
  openRemoteWindow("pista@msi", open);
  openRemoteWindow("pista@msi", open);
  assert.deepEqual(calls, [["/?remote=pista%40msi", "westerm-remote-pista%40msi", "popup,width=1280,height=820"]]);
  assert.equal(focused, 1);
});
```

Task 4의 `the ssh picker offers no westerm panes`는 새 시그니처(`openWindow` 자리에 `null`)로 고친다.

- [ ] **Step 2: 실패 확인**

Run: `node --import tsx --test tests/remote.test.ts`
Expected: FAIL — `openRemoteWindow`가 없음, `window-prod` 명령이 없음.

- [ ] **Step 3: 구현** — 위 Interfaces대로. `App.tsx`는 `REMOTE_TARGET`이 null일 때만 `openRemoteWindow`를 넘긴다. 원격 창 버튼의 아이콘은 `RemoteWindowIcon`.

- [ ] **Step 4: 통과 확인**

Run: `npm run type-check`, `node --import tsx --test tests/*.test.ts`, `npx vite build`
Expected: PASS.

- [ ] **Step 5: 체크포인트** — 제안: `git add src tests && git commit -m "Open a server in a new window from the palette and the activity bar"`

---

### Task 8: macOS 앱의 원격 창

**Files:**
- Modify: `macos/app/main.swift`

**Interfaces:**
- Consumes: Task 7의 `window.open("/?remote=…", "westerm-remote-…", "popup,width=1280,height=820")`.
- Produces: 대시보드 출처이고 `remote` 질의가 있는 새 창 요청은 앱 창이 된다. 그 밖의 주소는 지금처럼 기본 브라우저로 연다.

이 레포에는 앱의 자동 테스트가 없다. 빌드와 수동 확인으로 검증한다.

- [ ] **Step 1: 구현**

- `private var remoteWindows: [String: NSWindow]`, `private func remoteTarget(_ url: URL) -> String?` (host `127.0.0.1`, port `dashboard?.port`, `remote` 질의 항목).
- `createWebViewWith`: `remoteTarget`가 있으면, 그 대상의 창이 있을 때 `makeKeyAndOrderFront`하고 `nil`을 돌려준다. 없으면 받은 `configuration`으로 `WKWebView`를 만들고 (delegate는 self), 새 `NSWindow`에 넣는다: `isReleasedWhenClosed = false`, `setFrameAutosaveName("westerm remote \(target)")`, `tabbingMode = .disallowed`. 그 web view를 돌려준다 (WebKit이 요청을 그 안에서 연다). 대상이 없으면 지금처럼 `NSWorkspace.shared.open`.
- 원격 창 제목은 `webView.observe(\.title)`로 페이지 제목을 따라간다. 창이 닫히면(`NSWindowDelegate.windowWillClose`) 사전에서 지운다.
- 메인 창: `isReleasedWhenClosed = false`. `applicationShouldHandleReopen(_:hasVisibleWindows:)`에서 보이는 창이 없으면 메인 창을 다시 보여 준다.
- `webViewWebContentProcessDidTerminate`: 끝난 그 web view의 URL을 다시 연다 (메인 창의 주소로 바꾸지 않는다).

- [ ] **Step 2: 빌드**

Run: `sh macos/build-app.sh`
Expected: 빌드 성공, 스크립트가 알려 주는 위치에 `westerm.app`.

- [ ] **Step 3: 수동 확인** (Task 10의 개발 대시보드가 4318에서 돌 때)

Run: `PORT=4318 <빌드된 westerm.app>/Contents/MacOS/westerm`
Expected:
- 팔레트의 "새 창에서 열기: <서버>"로 새 앱 창이 열리고, 제목이 `westerm — <서버>`.
- 같은 명령을 다시 쓰면 새 창 대신 그 창이 앞으로 온다.
- 메인 창을 닫아도 원격 창은 남는다. Dock 아이콘을 누르면 메인 창이 다시 보인다.
- 원격 창을 닫아도 서버의 셸은 남는다 (다시 열면 같은 셸).

- [ ] **Step 4: 체크포인트** — 제안: `git add macos && git commit -m "Open remote windows as app windows in westerm.app"`

---

### Task 9: 문서와 변경 기록

**Files:**
- Modify: `docs/architecture.md`, `docs/sessions.md`, `docs/usage.md`, `CHANGELOG.md`

- [ ] **Step 1: 쓰기**

- `docs/architecture.md`: 구성 그림 (로컬 창과 원격 창, 로컬 대시보드의 `/remote/…`, gate → 서버 대시보드 → 서버 host). 경로 표에 `server/http1.rs`, `server/remote_window.rs`, `remote/daemon.rs`, `remote/dashboard.rs`. Security 절: 서버 대시보드는 Unix 소켓(0600)만, 원격 경로의 출처와 Host 확인, gate는 인증된 스트림만 서버 대시보드에 이음. Known gap: 로컬 프로그램이 원격 창 경로로 서버 셸을 열 수 있음. 테스트 절: `native/tests/remote.rs`가 다루는 것.
- `docs/sessions.md`: "Remote sessions" → 원격 창의 세션이 살아남는 경우 (창 닫기, 네트워크 끊김, 로컬 대시보드 재시작, 서버 재부팅 뒤 "revived"). 끊긴 동안 닫은 패인은 다시 연결될 때 서버에서 끝남.
- `docs/usage.md`: 활동 막대 줄을 고침. "Remote westerm" 절 → "Remote windows": 여는 법, 창 제목, 연결 표시(QUIC / SSH tunnel), 재연결과 "Retry", 필요한 것 표(그대로), host 교체 질문, 서버에서의 명령(`westerm stop`은 host, gate, 서버 대시보드를 멈춤), 클라이언트 해제, 변수 표, 버전이 다른 두 컴퓨터는 높은 버전이 이김, v0.2.0 원격 패인은 레이아웃에서 빠짐.
- `CHANGELOG.md` `[Unreleased]`: Added (원격 창), Removed (원격 westerm 패인; 저장된 원격 패인은 레이아웃에서 빠지고, 그 셸은 서버에서 `westerm stop`으로 끝남), Changed (gate가 서버 대시보드로 이음, `westerm stop`이 서버 대시보드도 멈춤, 업데이트 뒤 로컬 셸이 한 번 "restored").

- [ ] **Step 2: 확인**

Run: `grep -n "remote-windows\|Remote windows" docs/*.md` (앵커와 링크가 맞는지), `npm test`
Expected: 링크 대상이 있음. 테스트 PASS.

- [ ] **Step 3: 체크포인트** — 제안: `git add docs CHANGELOG.md && git commit -m "Document remote windows"`

---

### Task 10: 테스트 서버에서 종단 확인

서버: `pista@msi-linux-private-pista` (Ubuntu, x86_64, zsh, LAN 192.168.219.105).

- [ ] **Step 1: 빌드**

Run: `npm run build` (UI와 로컬 실행 파일), 그다음 `cargo zigbuild --release --target x86_64-unknown-linux-musl --manifest-path native/Cargo.toml`, 결과를 `/tmp/wrb/westerm-x86_64-unknown-linux-musl`로 복사
Expected: 두 빌드 성공.

- [ ] **Step 2: 개발 대시보드 띄우기**

Run: `WESTERM_STATE_DIR=/tmp/wq WESTERM_REMOTE_BINARY_DIR=/tmp/wrb native/target/release/westerm serve --port 4318` (백그라운드, 로그 `/tmp/wq-dashboard.log`)
Expected: `> westerm: http://127.0.0.1:4318`.

- [ ] **Step 3: 원격 창 열기** (Chrome)

- `http://127.0.0.1:4318/?remote=pista@msi-linux-private-pista`를 연다.
- Expected: 연결 화면에 단계가 보임 → 대시보드. 제목 `westerm — pista@msi-linux-private-pista`. 연결 표시는 `SSH tunnel` (이 프로세스는 Local Network 권한이 없어서 QUIC이 막힘) 또는 `QUIC`.
- 서버 host 교체 질문이 나오면, 답하기 전에 사용자에게 묻는다.

- [ ] **Step 4: 기능 확인**

- 탭을 하나 더 만들고 워크스페이스를 하나 더 만든다. terminal-browser와 terminal-code 패인을 연다.
- Expected: 두 패인이 서버에서 실행됨 (`ssh … 'pgrep -fa "terminal-browser|tode"'`). 서버의 `~/.local/state/westerm/workspace.json`에 새 탭과 워크스페이스가 있음.
- 창을 닫고 다시 연다. Expected: 같은 탭, 같은 셸 (`reattached`).
- 다른 클라이언트: 두 번째 개발 대시보드(`WESTERM_STATE_DIR=/tmp/wq2`, `--port 4319`)로 같은 서버를 연다. Expected: 같은 탭과 워크스페이스. 앞의 창은 "다른 창에서 사용 중"이 됨.

- [ ] **Step 5: 끊김 확인**

- 로컬 연결 프로세스(SSH 터널이면 그 `ssh -L` 프로세스)를 끝낸다.
- Expected: "재연결 중" 띠 → 스스로 다시 연결 → 같은 셸, 끊긴 동안의 출력이 보임.
- 끊긴 동안 패인 하나를 닫는다. Expected: 다시 연결된 뒤 그 셸이 서버에서 끝남.

- [ ] **Step 6: 사용자 확인 요청**

- macOS 앱(Task 8)의 원격 창과, Local Network 권한이 있는 터미널이나 앱에서의 QUIC 연결은 사용자에게 확인을 부탁한다.

- [ ] **Step 7: 정리**

- 이 작업이 만든 것만 정리한다: 개발 대시보드 두 개, `/tmp/wq`, `/tmp/wq2`, `/tmp/wrb`, 로그.
- 서버의 host와 gate를 멈추거나 서버의 파일을 지우는 것은 사용자에게 먼저 묻는다.
- 서버에는 개발 빌드가 남는다. 다음에 릴리스 빌드로 연결하면 그 빌드로 다시 설치된다고 알린다.
