# 원격 westerm (SSH 설치 + QUIC 연결) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** westerm이 SSH로 Linux 서버에 westerm을 설치하고, 그 서버의 셸을 QUIC으로 연결해서, 네트워크가 끊겨도 원격 셸이 살아남게 한다.

**Architecture:** 원격에서는 기존 PTY host를 그대로 쓰고, 새 `westerm gate`가 인증된 QUIC 스트림을 host의 Unix 소켓에 잇는다. `westerm remote-setup`은 SSH로 실행되어 host와 gate를 준비하고 접속 정보를 JSON 한 줄로 알려 준다. 로컬 대시보드의 `RemoteManager`가 서버마다 연결 하나(QUIC, 안 되면 `ssh -L` 터널)를 관리하고, 기존 WebSocket relay를 그 스트림 위에서 그대로 돌린다.

**Tech Stack:** Rust (tokio, axum, tokio-tungstenite, quinn 0.11.12, rustls 0.23.46 + ring, rcgen 0.14.10), TypeScript/React (wterm), node:test, cargo test.

**Spec:** `docs/superpowers/specs/2026-11-05-remote-quic-design.md`

## Global Constraints

- 원격: Linux x86_64 / aarch64, 릴리스의 musl 정적 실행 파일 (`westerm-x86_64-unknown-linux-musl`, `westerm-aarch64-unknown-linux-musl`).
- 로컬: macOS, Linux, Windows에서 `cargo build`와 `cargo test`가 통과해야 한다. gate, remote-setup, SSH 터널은 `#[cfg(unix)]`. Windows에서 `westerm gate`, `westerm remote-setup`은 "runs on Linux and macOS only"를 stderr에 쓰고 exit 2.
- 원격 설치 위치 `~/.local/bin/westerm`, 원격 상태 폴더는 기본값(`~/.local/state/westerm`, `WESTERM_STATE_DIR`가 있으면 그것).
- 기본 포트 UDP 4317, 사용 중이면 4318–4326 (`DEFAULT_PORT = 4317`, `PORT_SPAN = 10`). ALPN `westerm/1`. TLS 1.3만.
- 대시보드는 `127.0.0.1`에서만 듣는다. 새 API는 모두 기존 `origin_allowed`와 `Content-Type: application/json` 검사를 거친다.
- gate 코드(`native/src/remote/`, `native/src/server/`)는 `native/build.rs`의 `HOST_SOURCES`에 넣지 않는다.
- 원격 명령은 고정 문자열이다. 사용자 입력과 변하는 값은 stdin으로만 보낸다. 대상은 `normalize_ssh_target`으로 검증하고 `--` 뒤에 둔다. SSH 옵션은 `-T -o BatchMode=yes -o ConnectTimeout=10`.
- 고정 원격 명령은 sh, bash, zsh, fish에서 같은 뜻이어야 한다.
- 키, 인증서, `authorized_clients`, 기록 파일은 0600 (`state_dir::private_file()`), `remote/` 폴더는 0700.
- 다운그레이드 금지: 원격 버전이 로컬보다 높으면 설치하지 않는다.
- 다운로드는 시스템 `curl`과 `tar`로 한다 (`update.rs` 방식). TLS 라이브러리는 QUIC에만 쓴다.
- UI 문구는 `src/i18n/messages.ts`에 en, ko, ja, zh 네 언어로. 팔레트 `keywords`는 영어.
- `shared/protocol.ts`와 `native/src/protocol.rs`는 같은 태스크에서 함께 바꾼다.
- 레포 문서(`docs/`, `CHANGELOG.md`, `CONTRIBUTING*.md`)는 영어(CONTRIBUTING.ko.md는 한국어).
- 커밋 금지: 에이전트는 `git commit`/`git push`를 실행하지 않는다. 각 태스크 끝의 "체크포인트"에서 커밋 명령을 제안만 한다. 커밋 메시지에 `Co-Authored-By` 줄을 넣지 않는다. 메시지는 레포 관례대로 명령형 영어 한 줄.
- 개발 중에는 사용자의 westerm(포트 4317, `~/.local/state/westerm`)을 건드리지 않는다. 수동 실행은 `WESTERM_STATE_DIR=/tmp/wq`와 `--port 4318`.

## Review Focus

1. 원격 로그인 셸이 bash가 아닐 때(테스트 서버는 zsh, 다른 서버는 fish): 고정 원격 명령이 그대로 동작해야 한다. → Task 6 `fixed_commands_work_in_every_available_shell`.
2. 저장된 레이아웃을 열어서 같은 서버의 패인 여러 개가 동시에 연결할 때: 확인과 설치는 한 번만 해야 한다. → Task 8 `two_panes_opening_at_once_install_once`.
3. 연결 중이거나 끊긴 원격 패인을 닫을 때: 원격 셸도 끝나야 한다. → Task 8 `closing_a_pane_while_connecting_ends_it_on_the_server`, Task 9 `remote panes report a close the socket cannot carry`.
4. QUIC 연결이 조용히 죽을 때(gate 종료, 잠자기): 몇 초 안에 알아채고 같은 셸로 돌아와야 한다. → Task 8 `a_lost_gate_comes_back_and_the_shell_is_reattached`.
5. 서버에 더 새 westerm이 있을 때: 덮어쓰지 않고 이유를 보여 줘야 한다. → Task 7 `never_downgrades`, Task 9 문구 `remote.failed.newerRemote`.

## File Map

| 파일 | 책임 |
|---|---|
| `native/build.rs` | host 빌드 id 해시 전에 CRLF → LF |
| `native/src/main.rs` | `version_line()`, 새 명령 `gate`, `remote-setup [--replace-host]`, `mod remote;` |
| `native/src/protocol.rs` | `PROTOCOL`, `SessionKind::Remote`, `RemoteStage` 등 원격 메시지, `CLOSE_REMOTE_FAILED` |
| `native/src/spawn.rs` | `Remote`는 로컬에서 실행하지 않음 |
| `native/src/host/mod.rs` | `/health`에 `protocol` |
| `native/src/remote/mod.rs` | 원격 공용 상수, `remote_dir()` |
| `native/src/remote/identity.rs` | 자체 서명 인증서와 키, 지문 |
| `native/src/remote/authorized.rs` | `authorized_clients` 읽기, 추가 |
| `native/src/remote/tls.rs` | 고정 지문 검증기, quinn 서버/클라이언트 설정 |
| `native/src/remote/report.rs` | `remote-setup` 출력 JSON |
| `native/src/remote/gate.rs` | `westerm gate`: QUIC 서버, 요청 검사, host로 중계 |
| `native/src/remote/setup.rs` | `westerm remote-setup`: 등록, host와 gate 준비, 보고 |
| `native/src/server/mod.rs` | relay를 `pipe()`로 분리, `remote_relay()`, 새 API 3개, `stop_host()`가 gate도 멈춤 |
| `native/src/server/host_client.rs` | `Health`, `health()`, `start_host()`를 crate에 공개 |
| `native/src/server/remote/mod.rs` | `Failure`, 모듈 묶음 |
| `native/src/server/remote/ssh.rs` | ssh 명령, 출력 해석, 오류 분류 |
| `native/src/server/remote/binary.rs` | Linux 실행 파일 고르기, 설치 계획 |
| `native/src/server/remote/records.rs` | 서버별 접속 기록, 끊긴 동안 닫은 세션 |
| `native/src/server/remote/quic.rs` | QUIC 클라이언트 연결 |
| `native/src/server/remote/tunnel.rs` | `ssh -L` 터널 (unix) |
| `native/src/server/remote/manager.rs` | `RemoteManager`: 서버별 상태, 준비 순서, host 교체 판단 |
| `native/src/update.rs` | `download_release_binary()` |
| `native/tests/common/mod.rs` | 통합 테스트 공용 도우미 (integration.rs에서 옮김) |
| `native/tests/remote.rs` | gate, remote-setup, 대시보드 원격 패인 통합 테스트 (unix) |
| `shared/protocol.ts`, `shared/workspace-file.ts` | TS 프로토콜과 저장 검증 |
| `src/remoteApi.ts` | 원격 API 호출 도우미 |
| `src/kinds.tsx`, `src/i18n/messages.ts`, `src/workspace.ts`, `src/terminal/connection.ts`, `src/components/Pane.tsx`, `src/App.tsx`, `src/paletteCommands.tsx`, `src/styles.css` | 원격 패인 UI |
| `tests/remote.test.ts` | UI 로직 테스트 |
| `docs/*.md`, `CHANGELOG.md`, `CONTRIBUTING*.md` | 문서 |

---

### Task 1: 버전 줄, CRLF에 흔들리지 않는 host 빌드 id, `/health`의 `protocol`

**Files:**
- Modify: `native/build.rs`, `native/src/main.rs`, `native/src/protocol.rs`, `native/src/host/mod.rs`, `native/src/server/host_client.rs`, `native/src/server/mod.rs` (`pub(crate) mod host_client;`)
- Test: `native/src/main.rs`, `native/src/server/host_client.rs` (unit), `native/tests/integration.rs`

**Interfaces:**
- Produces:
  - `crate::version_line() -> String` = `format!("westerm {} (host {})", env!("CARGO_PKG_VERSION"), host::HOST_BUILD)`. `--version`은 `"{version_line()} — Web Session Terminal, built on wterm"`을 출력한다 (지금과 같은 글자).
  - `crate::protocol::PROTOCOL: u32 = 1` — 대시보드와 host 사이 통신 규칙의 번호.
  - `server::host_client`: `pub(crate) struct Health { pub build: String, pub protocol: u32, pub sessions: usize }` (`protocol`, `sessions`는 없으면 0), `pub(crate) fn parse_health(body: &str) -> Option<Health>`, `pub(crate) async fn health(dir: &Path) -> Option<Health>`, `pub(crate) async fn start_host(dir: &Path) -> Result<(), String>` (오늘 `start()`의 실행하고 기다리는 부분; `start()`는 이것을 부른다), `pub(crate) async fn stop_and_wait(dir: &Path)`, `pub(crate) fn spawn_detached(command: &mut std::process::Command) -> std::io::Result<std::process::Child>`.

- [ ] **Step 1: 실패하는 테스트 쓰기**

```rust
// host_client.rs tests
#[test]
fn reads_health_with_and_without_protocol() {
    let h = parse_health(r#"{"build":"rs-1","pid":3,"sessions":2,"protocol":1}"#).unwrap();
    assert_eq!((h.build.as_str(), h.protocol, h.sessions), ("rs-1", 1, 2));
    assert_eq!(parse_health(r#"{"build":"rs-0","pid":3,"sessions":0}"#).unwrap().protocol, 0);
    assert!(parse_health("x").is_none());
}
// main.rs tests
#[test]
fn version_line_names_the_version_and_host_build() {
    assert!(version_line().starts_with(&format!("westerm {} (host rs-", env!("CARGO_PKG_VERSION"))));
    assert!(version_line().ends_with(')'));
}
// integration.rs
#[tokio::test]
async fn health_reports_the_protocol() {
    // start_host → http(GET /health) → body JSON의 "protocol" == 1, "build"는 "rs-"로 시작
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test --manifest-path native/Cargo.toml health version_line` → FAIL (`parse_health`, `version_line` 없음)
- [ ] **Step 3: 구현** — `build.rs`는 각 파일 바이트에서 `\r\n`을 `\n`으로 바꾼 뒤 해시한다. `host/mod.rs`의 `health`에 `"protocol": PROTOCOL`을 더한다. `running_build()`는 `health()`를 쓴다.
- [ ] **Step 4: 통과 확인** — Run: `cargo test --manifest-path native/Cargo.toml` → 모두 PASS
- [ ] **Step 5: 체크포인트** — 제안: `git add native && git commit -m "Report the host protocol and hash host sources with LF line ends"`

---

### Task 2: 프로토콜에 `remote` 종류와 원격 단계 메시지 넣기 (Rust + TS)

**Files:**
- Modify: `native/src/protocol.rs`, `native/src/spawn.rs` (unix와 windows `command_for` 둘 다), `shared/protocol.ts`, `shared/workspace-file.ts`
- Test: `native/src/protocol.rs`, `native/src/spawn.rs` (unit), Create `tests/remote.test.ts`

**Interfaces:**
- Produces (Rust, `crate::protocol`):
  - `SessionKind::Remote` (`"remote"`), `is_text()`는 `Remote`도 true.
  - `AttachRequest::from_query`: `Remote`도 `Ssh`처럼 `target`이 없으면 `None`.
  - `AttachRequest::for_remote_host(&self) -> AttachRequest`: 같은 `id`, `size`, `lang`; `kind = Shell`; `url`, `target`, `cwd_from`은 `None`.
  - `pub const CLOSE_REMOTE_FAILED: u16 = 4003;` — 다시 연결해도 소용없는 실패.
  - `#[serde(rename_all = "camelCase")] pub enum RemoteVia { Quic, Tunnel }`
  - `#[serde(rename_all = "camelCase")] pub enum RemoteFailure { Auth, HostKey, Unreachable, Unsupported, Download, DevBinary, NewerRemote, Incompatible, Transport, Setup }`, `pub fn is_permanent(self) -> bool` (`Unreachable`, `Transport`만 false).
  - `#[serde(tag = "stage", rename_all = "camelCase", rename_all_fields = "camelCase")] pub enum RemoteStage { Checking, Installing { version: String }, Starting, Connecting, Confirm { sessions: usize }, Connected { via: RemoteVia, kill_on_logout: bool }, Failed { reason: RemoteFailure, detail: String } }` (`Clone, Debug, PartialEq, Serialize`)
  - `ServerMessage::Remote(RemoteStage)` → `{"type":"remote","stage":…}`.
- Produces (TS, `shared/protocol.ts`): `SessionKind`에 `"remote"`, `RemoteVia`, `RemoteFailure`, `RemoteStage`(위와 같은 모양), `ServerMessage`에 `({ type: "remote" } & RemoteStage)`, `isPermanentFailure(reason: RemoteFailure): boolean`, `CLOSE_REMOTE_FAILED = 4003`, `REMOTE_DECISION_PATH = "/api/remote/decision"` (POST `{target, replace}`), `REMOTE_CLOSE_PATH = "/api/remote/close"` (POST `{target, session}`), `REMOTE_UPDATE_HOST_PATH = "/api/remote/update-host"` (POST `{target}`).
- `shared/workspace-file.ts`: `ssh`와 `remote`는 `target`이 없으면 거부.

- [ ] **Step 1: 실패하는 테스트 쓰기**

```rust
#[test]
fn remote_needs_a_destination_and_becomes_a_plain_shell_remotely() {
    let mut q = query(); // 기존 테스트의 기본 질의
    q.insert("kind".into(), "remote".into());
    assert!(AttachRequest::from_query(&q).is_none(), "remote needs a destination");
    q.insert("target".into(), "prod".into());
    let request = AttachRequest::from_query(&q).unwrap();
    let remote = request.for_remote_host();
    assert_eq!((remote.kind, remote.target, remote.url, remote.cwd_from), (SessionKind::Shell, None, None, None));
    assert_eq!(remote.id, request.id);
    assert!(remote.to_query().contains("kind=shell") && !remote.to_query().contains("target"));
}
#[test]
fn remote_stages_serialize_for_the_browser() {
    let json = |stage| ServerMessage::Remote(stage).to_json();
    assert_eq!(json(RemoteStage::Connected { via: RemoteVia::Quic, kill_on_logout: false }),
        r#"{"type":"remote","stage":"connected","via":"quic","killOnLogout":false}"#);
    assert_eq!(json(RemoteStage::Failed { reason: RemoteFailure::HostKey, detail: "x".into() }),
        r#"{"type":"remote","stage":"failed","reason":"hostKey","detail":"x"}"#);
    assert_eq!(json(RemoteStage::Installing { version: "0.2.0".into() }), r#"{"type":"remote","stage":"installing","version":"0.2.0"}"#);
    assert_eq!(json(RemoteStage::Confirm { sessions: 3 }), r#"{"type":"remote","stage":"confirm","sessions":3}"#);
}
#[test]
fn only_transient_remote_failures_retry() {
    assert!(!RemoteFailure::Unreachable.is_permanent() && !RemoteFailure::Transport.is_permanent());
    assert!(RemoteFailure::Auth.is_permanent() && RemoteFailure::NewerRemote.is_permanent());
}
// spawn.rs
#[test]
fn remote_sessions_never_start_locally() {
    assert!(command_for(SessionKind::Remote, None, Some("prod")).is_err());
}
```

```ts
// tests/remote.test.ts
test("saved remote sessions need a valid destination", () => {
  // tests/ssh.test.ts의 layout() 도우미와 같은 모양, kind: "remote"
  assert.equal(layout({ kind: "remote", target: "prod" })?.sessions[0].target, "prod");
  assert.equal(layout({ kind: "remote" }), null);
});
test("remote is a session kind and only some failures are permanent", () => {
  assert.equal(isSessionKind("remote"), true);
  assert.equal(isPermanentFailure("auth"), true);
  assert.equal(isPermanentFailure("unreachable"), false);
  assert.equal(isPermanentFailure("transport"), false);
});
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test --manifest-path native/Cargo.toml remote` 와 `node --import tsx --test tests/remote.test.ts` → FAIL
- [ ] **Step 3: 구현** — 위 Interfaces 그대로. `spawn.rs`의 `Remote` 분기는 `Err("remote sessions run on the remote host".into())`. TS에서 `Record<SessionKind, …>`를 쓰는 곳은 `npm run type-check`가 알려 준다. type-check가 통과하도록 그곳만 최소로 채운다: `src/workspace.ts`의 `TITLE_PREFIX.remote = "Remote"`, `src/kinds.tsx`의 `KINDS.remote = { icon: ServerCog, imageLimit: 32 * MiB }` (`LAUNCHERS`는 Task 9에서), `src/App.tsx`의 `counts`에 `remote: 0`, `src/i18n/messages.ts`의 `kinds.remote` 네 언어 (값은 Task 9 표).
- [ ] **Step 4: 통과 확인** — Run: `cargo test --manifest-path native/Cargo.toml` → PASS, `npm test && npm run type-check` → PASS
- [ ] **Step 5: 체크포인트** — 제안: `git commit -m "Add the remote session kind and remote stage messages to the protocol"`

---

### Task 3: 인증서, 허용 목록, QUIC/TLS 설정

**Files:**
- Modify: `native/Cargo.toml`:
  ```toml
  quinn = { version = "0.11.12", default-features = false, features = ["runtime-tokio", "rustls-ring"] }
  rustls = { version = "0.23.46", default-features = false, features = ["ring", "std"] }
  rcgen = { version = "0.14.10", default-features = false, features = ["crypto", "ring"] }
  ```
- Modify: `native/src/main.rs` (`mod remote;`)
- Create: `native/src/remote/mod.rs`, `identity.rs`, `authorized.rs`, `tls.rs`, `report.rs`
- Test: 각 파일의 unit test

**Interfaces:**
- Produces:
  - `remote/mod.rs`: `pub const ALPN: &[u8] = b"westerm/1"`, `DEFAULT_PORT: u16 = 4317`, `PORT_SPAN: u16 = 10`, `KEEP_ALIVE: Duration = 4 s`, `IDLE_TIMEOUT: Duration = 15 s`, `HANDSHAKE_TIMEOUT: Duration = 3 s`, `pub fn remote_dir(state: &Path) -> io::Result<PathBuf>` (`state/remote`, 만들고 unix에서 0700).
  - `identity.rs`: `pub struct Identity { pub cert: CertificateDer<'static>, pub key: PrivatePkcs8KeyDer<'static> }`, `pub fn load_or_create(dir: &Path, name: &str) -> io::Result<Identity>` (`<name>.crt`, `<name>.key`, DER, 0600, ECDSA P-256, SAN `westerm`), `pub fn fingerprint(cert_der: &[u8]) -> String` (SHA-256 소문자 hex 64자, `sha2` 사용), `pub fn is_fingerprint(text: &str) -> bool`.
  - `authorized.rs`: `pub fn read(path: &Path) -> Vec<String>` (올바른 지문 줄만, 파일 순서, 파일 없으면 빈 목록), `pub fn add(path: &Path, fingerprint: &str) -> io::Result<bool>` (이미 있으면 false, 틀린 지문은 `InvalidInput`, 새 파일은 0600).
  - `tls.rs`: `pub fn server_config(identity: &Identity, authorized: PathBuf) -> Result<quinn::ServerConfig, String>`, `pub fn client_config(identity: &Identity, server_fingerprint: &str) -> Result<quinn::ClientConfig, String>`. 내부 검증기: 서버 인증서는 지문이 고정 값과 같을 때만, 클라이언트 인증서는 `authorized`를 핸드셰이크마다 다시 읽어서 지문이 있을 때만 통과. 서명 검증은 `rustls::crypto::verify_tls13_signature`와 ring provider의 `signature_verification_algorithms`. 양쪽 모두 ALPN `westerm/1`, TLS 1.3만, transport는 `KEEP_ALIVE`, `IDLE_TIMEOUT`. 클라이언트 인증은 필수.
  - `report.rs`: `pub const SETUP_VERSION: u32 = 1`, `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)] pub struct SetupReport { pub v: u32, pub version: String, pub port: u16, pub fingerprint: String, pub socket: String, pub host: HostInfo, #[serde(rename = "killOnLogout")] pub kill_on_logout: bool }`, `pub struct HostInfo { pub build: String, pub protocol: u32, pub sessions: usize }` (같은 derive), `pub fn parse_report(stdout: &str) -> Option<SetupReport>` (마지막 비어 있지 않은 줄, `v == SETUP_VERSION`일 때만).

- [ ] **Step 1: 실패하는 테스트 쓰기**

```rust
// identity.rs
#[test] fn creates_once_and_reloads_the_same_identity() { /* 두 번 load_or_create → 같은 fingerprint; unix에서 .key 권한 0o600 */ }
#[test] fn fingerprints_are_64_lowercase_hex() {
    let id = load_or_create(dir, "c").unwrap();
    assert!(is_fingerprint(&fingerprint(&id.cert)));
    assert!(!is_fingerprint(&"A".repeat(64)) && !is_fingerprint(&"a".repeat(63)) && !is_fingerprint(&"g".repeat(64)));
}
// authorized.rs
#[test] fn adds_each_fingerprint_once() {
    assert!(add(&path, &fp).unwrap()); assert!(!add(&path, &fp).unwrap());
    std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"junk\n").unwrap();
    assert_eq!(read(&path), vec![fp.clone()]);
    assert_eq!(add(&path, "nope").unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
}
// tls.rs — 127.0.0.1:0에서 quinn 서버와 클라이언트를 같은 프로세스에 띄운다
#[tokio::test] async fn an_authorized_client_reaches_the_pinned_server() { /* open_bi → "ping" 왕복 */ }
#[tokio::test] async fn a_client_refuses_a_server_with_another_certificate() { /* 다른 지문 고정 → connect 오류 */ }
#[tokio::test] async fn the_server_refuses_an_unknown_client() { /* 빈 authorized → 연결 실패 */ }
#[tokio::test] async fn the_server_rereads_the_authorized_file() { /* 빈 파일 → 실패, add 후 같은 서버 설정으로 새 연결 → 성공 */ }
// report.rs
#[test] fn reads_the_last_line_as_the_report() {
    // "> pty host … started\n{json}\n" → Some; v:2 → None; "garbage" → None
    // 직렬화 결과에 "killOnLogout" 키가 있다
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test --manifest-path native/Cargo.toml remote::` → FAIL
- [ ] **Step 3: 구현** — Interfaces 그대로. rustls 설정은 `builder_with_provider(Arc::new(rustls::crypto::ring::default_provider())).with_protocol_versions(&[&rustls::version::TLS13])`. quinn은 `quinn::crypto::rustls::QuicServerConfig::try_from` / `QuicClientConfig::try_from`.
- [ ] **Step 4: 통과 확인** — Run: `cargo test --manifest-path native/Cargo.toml` → PASS. `native/build.rs`의 `HOST_SOURCES`가 그대로인지 확인.
- [ ] **Step 5: 체크포인트** — 제안: `git commit -m "Add certificates, the authorized client list, and QUIC TLS settings"`

---

### Task 4: `westerm gate`

**Files:**
- Create: `native/src/remote/gate.rs` (`#[cfg(unix)] pub mod gate;`)
- Modify: `native/src/main.rs` (명령 `gate`), `native/src/server/mod.rs` (`stop_host()`가 unix에서 gate도 멈춤)
- Test: `native/src/remote/gate.rs` (unit)

**Interfaces:**
- Consumes: Task 3 전부, `crate::ipc::connect`, `crate::state_dir::{state_dir, private_file}`, `crate::version_line`.
- Produces:
  - `pub async fn run(state: &Path) -> io::Result<()>` — `westerm gate`.
  - `pub fn request_allowed(head: &str) -> bool` — `head`는 요청 줄과 헤더(빈 줄 전까지). 요청 줄이 정확히 `GET /session?<공백 없는 나머지> HTTP/1.1`이고, `upgrade: websocket` 헤더(이름과 값 모두 대소문자 무시)가 있을 때만 true.
  - `#[derive(Serialize, Deserialize)] pub struct GateRecord { pub pid: u32, pub port: u16, pub version: String }` — 상태 폴더의 `gate.json` (임시 파일 → rename, 0600).
  - `pub fn read_record(state: &Path) -> Option<GateRecord>`, `pub fn is_running(state: &Path) -> bool` (`gate.lock`을 잠글 수 없으면 true), `pub async fn stop_running(state: &Path) -> io::Result<bool>` (돌고 있으면 기록된 pid에 SIGTERM, 잠금이 풀릴 때까지 최대 5초; 아니면 낡은 `gate.json`을 지우고 false).
  - `pub fn ports_from(env: Option<&str>) -> Vec<u16>` — `None`이나 숫자가 아니면 4317..=4326, `Some("0")` → `[0]`(아무 빈 포트), `Some("5000")` → `[5000]`. `run`은 `WESTERM_GATE_PORT`를 넘긴다.
  - `pub fn bind_first(config: quinn::ServerConfig, ports: &[u16]) -> io::Result<(quinn::Endpoint, u16)>` — 포트마다 `[::]:p`, 그다음 `0.0.0.0:p`를 시도. 모두 실패하면 "no free UDP port in 4317–4326".
- 동작:
  - `gate.lock`을 `File::try_lock`으로 잡는다 (host와 같은 방식). 못 잡으면 "another gate is already running" 오류.
  - 신원 `load_or_create(remote_dir(state), "gate")`, 허용 목록 `remote_dir(state)/authorized_clients`.
  - 받기: `remote_address_validated()`가 아니면 `retry()`(QUIC 주소 검증). 연결마다 `accept_bi()`를 반복하고 스트림마다 task.
  - 스트림 중계: (1) 클라이언트 헤더를 `\r\n\r\n`까지 읽는다 (최대 8 KiB, 10초). (2) `request_allowed`가 아니면 스트림을 `reset`하고 끝. (3) host에 연결하고 읽은 바이트를 그대로 쓴다. (4) host 응답 헤더를 `\r\n\r\n`까지 읽어서 클라이언트에 쓴다. (5) 상태 줄이 `HTTP/1.1 101`이면 `tokio::io::copy_bidirectional(&mut tokio::io::join(recv, send), &mut host)`, 아니면 끝. 이렇게 하면 업그레이드에 실패한 연결 뒤에 붙인 다른 요청(예: `POST /shutdown`)이 host에 닿지 않는다.
  - SIGTERM, SIGINT, SIGHUP → endpoint를 닫고 `gate.json`을 지우고 exit 0.
  - `westerm stop`: host를 멈춘 뒤 `stop_running(&state_dir()?)`. true면 "westerm: the remote gate stopped" 출력.

- [ ] **Step 1: 실패하는 테스트 쓰기**

```rust
#[test]
fn only_websocket_session_upgrades_pass() {
    let ok = "GET /session?session=sa&kind=shell&cols=80 HTTP/1.1\r\nHost: host\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n";
    assert!(request_allowed(ok));
    assert!(request_allowed(&ok.replace("Upgrade: websocket", "upgrade: WebSocket")));
    for bad in [
        "GET /session?session=sa HTTP/1.1\r\nHost: host\r\n",          // 업그레이드 아님
        "POST /shutdown HTTP/1.1\r\nHost: host\r\n",
        "GET /health HTTP/1.1\r\nUpgrade: websocket\r\n",
        "GET /session HTTP/1.1\r\nUpgrade: websocket\r\n",
        "GET /session?x HTTP/1.0\r\nUpgrade: websocket\r\n",
        "GET /session?x y HTTP/1.1\r\nUpgrade: websocket\r\n",
        "",
    ] { assert!(!request_allowed(bad), "{bad:?}"); }
}
#[test]
fn ports_follow_the_override() {
    assert_eq!(ports_from(None), (4317..=4326).collect::<Vec<_>>());
    assert_eq!(ports_from(Some("0")), vec![0]);
    assert_eq!(ports_from(Some("5000")), vec![5000]);
    assert_eq!(ports_from(Some("x")), ports_from(None));
}
#[tokio::test]
async fn binds_the_next_port_when_one_is_taken() {
    // 빈 포트 p를 찾고 [::]:p와 0.0.0.0:p를 std::net::UdpSocket으로 잡아 둔다 (두 번째 실패는 무시)
    // bind_first(server_config, &[p, 0]) → 반환 포트 != p
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test --manifest-path native/Cargo.toml gate` → FAIL
- [ ] **Step 3: 구현** — Interfaces와 동작 그대로. `main.rs`의 usage 문자열에 `westerm gate`, `westerm remote-setup [--replace-host]`를 더한다 (Task 5 명령도 여기서 자리만 만든다).
- [ ] **Step 4: 통과 확인** — Run: `cargo test --manifest-path native/Cargo.toml` → PASS. `cargo check --manifest-path native/Cargo.toml --target x86_64-pc-windows-msvc`로 Windows 빌드가 깨지지 않았는지 확인 (타깃은 설치돼 있다; 링크까지는 하지 않음).
- [ ] **Step 5: 체크포인트** — 제안: `git commit -m "Add westerm gate, a QUIC entry to the PTY host"`

---

### Task 5: `westerm remote-setup`과 gate 종단 테스트

**Files:**
- Create: `native/src/remote/setup.rs` (`#[cfg(unix)] pub mod setup;`)
- Modify: `native/src/main.rs` (`remote-setup [--replace-host]`)
- Create: `native/tests/common/mod.rs` — `integration.rs`의 `BIN`, `Spawned`, `StopHostOnDrop`을 옮기고 두 테스트 파일에서 `mod common;`으로 쓴다.
- Create: `native/tests/remote.rs` (`#![cfg(unix)]`)

**Interfaces:**
- Consumes: Task 1 `host_client::{health, start_host, stop_and_wait, spawn_detached}`, Task 3, Task 4 `gate::{read_record, is_running, stop_running}`.
- Produces: `pub async fn run(replace_host: bool) -> io::Result<()>`; `fn kills_user_processes(logind_conf: &str) -> bool` (주석이 아닌 마지막 `KillUserProcesses=` 값이 `yes`); `fn linger_enabled(loginctl: &str) -> bool` (`Linger=yes`).
- 순서:
  1. stdin 한 줄을 읽는다. `is_fingerprint`가 아니면 stderr에 "expected a client fingerprint"를 쓰고 exit 2.
  2. `authorized::add(remote_dir/authorized_clients, fp)`.
  3. `health(dir)`가 없으면 `start_host(dir)`. 빌드가 `HOST_BUILD`와 다르고 `replace_host`면 `stop_and_wait` 후 `start_host`. 다르고 `replace_host`가 아니면 그대로 둔다.
  4. gate: `is_running && record.version == version_line()`이면 그대로. 아니면 `stop_running` 후 `current_exe() gate`를 `spawn_detached`로 실행 (stdin null, stdout/stderr는 `gate.log`에 append, 0600). 최대 5초 동안 `read_record`의 pid가 새 child pid가 될 때까지 기다린다.
  5. `kill_on_logout = kills_user_processes(/etc/systemd/logind.conf와 logind.conf.d/*.conf 내용) && !linger_enabled(loginctl show-user <uid> --property=Linger 출력)`. 파일이나 명령이 없으면 false.
  6. stdout 마지막 줄에 `SetupReport` JSON: `v = 1`, `version = version_line()`, `port`/`fingerprint`는 gate 기록과 gate 인증서, `socket = ipc::describe(&dir)`, `host = health(dir)` (3 이후의 상태).

- [ ] **Step 1: 실패하는 테스트 쓰기**

```rust
// setup.rs unit
#[test]
fn reads_logind_settings() {
    assert!(!kills_user_processes("#KillUserProcesses=yes\n"));
    assert!(kills_user_processes("[Login]\nKillUserProcesses=yes\n"));
    assert!(!kills_user_processes("KillUserProcesses=yes\nKillUserProcesses=no\n"));
    assert!(linger_enabled("Linger=yes\n") && !linger_enabled("Linger=no\n"));
}
```

`native/tests/remote.rs`의 테스트 도우미 (테스트 안에서 직접 구현):
- `ClientId::new()` — rcgen으로 클라이언트 인증서를 만들고 지문을 계산한다.
- `setup(state: &Path, id: &ClientId, extra: &[&str]) -> serde_json::Value` — `BIN remote-setup`을 `WESTERM_STATE_DIR=state`, `WESTERM_GATE_PORT=0`, `SHELL=/bin/sh`로 실행하고 stdin에 지문을 쓰고, stdout 마지막 줄을 JSON으로 읽는다.
- `quic(port, id, expected_fp) -> Result<quinn::Connection, _>` — 모든 서버 인증서를 받는 검증기로 연결한 뒤, `peer_identity()`의 인증서 지문이 `expected_fp`와 같은지 확인한다.
- `attach(conn, query) -> (WebSocketStream<Join<RecvStream, SendStream>>, ready JSON)` — `tokio_tungstenite::client_async("ws://host/session?{query}", tokio::io::join(recv, send))`, 그다음 `integration.rs`의 `Viewer`와 같은 순서(크기 → ready).

```rust
#[tokio::test]
async fn setup_starts_a_gate_that_relays_a_shell() {
    // report: v==1, host.protocol==1, host.build == BIN --version의 host id, fingerprint 64 hex, socket이 "host.sock"로 끝남
    // quic → attach(session=sr1&kind=shell&…) → ready.mode=="new" → MARK 입력 → 출력에 MARK_42
    // 연결을 닫고 새 연결 → attach → ready.mode=="reattached", 다시 보낸 화면에 MARK_42
}
#[tokio::test]
async fn setup_twice_keeps_one_gate_and_one_authorized_line() {
    // 두 번 setup → port, fingerprint 같음; gate.json pid 같음; remote/authorized_clients 줄 수 == 1
}
#[tokio::test]
async fn the_gate_refuses_unknown_clients_and_other_requests() {
    // 등록 안 된 ClientId → quic 실패 또는 attach 실패
    // 등록된 클라이언트가 스트림에 "POST /shutdown HTTP/1.1\r\nHost: host\r\n\r\n" → "HTTP/1.1"을 받지 못하고 스트림이 끝남
    // "GET /session?bad HTTP/1.1\r\nUpgrade: websocket\r\n…\r\n\r\nPOST /shutdown HTTP/1.1\r\n…" → host는 여전히 Unix 소켓의 /health에 답함
}
#[tokio::test]
async fn stop_ends_the_host_and_the_gate() {
    // setup 후 BIN stop → 5초 안에 gate.json과 host.sock이 사라짐
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test --manifest-path native/Cargo.toml --test remote` → FAIL
- [ ] **Step 3: 구현** — 위 순서 그대로. 상태 폴더는 짧은 경로로 만든다 (`tempfile::Builder::new().prefix("wr").tempdir_in("/tmp")`; Unix 소켓 104바이트 제한). 테스트마다 `StopHostOnDrop`을 둔다 (Task 4 이후 `stop`은 gate도 멈춘다).
- [ ] **Step 4: 통과 확인** — Run: `cargo test --manifest-path native/Cargo.toml` → PASS (integration.rs도 common 모듈로 계속 통과)
- [ ] **Step 5: 체크포인트** — 제안: `git commit -m "Add westerm remote-setup and test the gate end to end"`

---

### Task 6: 대시보드의 SSH 명령

**Files:**
- Create: `native/src/server/remote/mod.rs`, `native/src/server/remote/ssh.rs`
- Modify: `native/src/server/mod.rs` (`mod remote;`)
- Test: `native/src/server/remote/ssh.rs` (unit)

**Interfaces:**
- Produces:
  - `server::remote::Failure { pub reason: RemoteFailure, pub detail: String }`, `Failure::new(reason, detail: impl Into<String>)`.
  - 고정 원격 명령 (글자 그대로):
    - `PROBE = "uname -sm; ~/.local/bin/westerm --version 2>/dev/null || true"`
    - `UPLOAD = "mkdir -p ~/.local/bin && cat > ~/.local/bin/.westerm.upload && chmod 755 ~/.local/bin/.westerm.upload && mv -f ~/.local/bin/.westerm.upload ~/.local/bin/westerm"`
    - `SETUP = "~/.local/bin/westerm remote-setup"`, `SETUP_REPLACE = "~/.local/bin/westerm remote-setup --replace-host"`
  - `pub struct Probe { pub os: String, pub arch: String, pub version: Option<String> }`, `pub fn parse_probe(stdout: &str) -> Option<Probe>` — 1줄: 단어 두 개 `<os> <arch>`. 2줄(있으면): `westerm `으로 시작할 때만 ` — ` 앞부분이 `version`.
  - `pub struct Resolved { pub hostname: String, pub proxied: bool }`, `pub fn parse_config(stdout: &str) -> Option<Resolved>` — `ssh -G` 출력의 `hostname` 줄. `proxyjump`나 `proxycommand` 값이 `none`이 아니면 `proxied`.
  - `pub fn classify(stderr: &str) -> RemoteFailure` — "Permission denied" → `Auth`; "Host key verification failed" 또는 "host key" → `HostKey`; "Could not resolve hostname", "Connection timed out", "Operation timed out", "Connection refused", "No route to host", "Network is unreachable" → `Unreachable`; 나머지 `Setup`.
  - `pub fn command_args(target: &str, remote_command: &str) -> Vec<String>` = `["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "--", target, remote_command]`.
  - `pub async fn run(target: &str, remote_command: &str, stdin: Option<Vec<u8>>) -> Result<String, Failure>` — `tokio::process::Command::new("ssh")`, `kill_on_drop(true)`, stdin 없으면 `Stdio::null()`. 0이 아닌 종료 → `Failure(classify(stderr), stderr 마지막 300자)`.
  - `pub async fn resolve(target: &str) -> Result<Resolved, Failure>` — `ssh -G -- <target>`.

- [ ] **Step 1: 실패하는 테스트 쓰기**

```rust
#[test]
fn probe_output_gives_os_arch_and_version() {
    let p = parse_probe("Linux x86_64\nwesterm 0.2.0 (host rs-1) — Web Session Terminal, built on wterm\n").unwrap();
    assert_eq!((p.os.as_str(), p.arch.as_str(), p.version.as_deref()), ("Linux", "x86_64", Some("westerm 0.2.0 (host rs-1)")));
    assert_eq!(parse_probe("Linux aarch64\n").unwrap().version, None);
    assert_eq!(parse_probe("Linux x86_64\nzsh: no such file\n").unwrap().version, None);
    assert!(parse_probe("").is_none());
}
#[test]
fn ssh_config_gives_the_host_and_whether_it_is_proxied() {
    let r = parse_config("user pista\nhostname 192.168.219.105\nport 22\nproxyjump none\n").unwrap();
    assert_eq!((r.hostname.as_str(), r.proxied), ("192.168.219.105", false));
    assert!(parse_config("hostname h\nproxyjump bastion\n").unwrap().proxied);
    assert!(parse_config("hostname h\nproxycommand ssh -W %h:%p jump\n").unwrap().proxied);
    assert!(parse_config("port 22\n").is_none());
}
#[test]
fn ssh_errors_are_classified() {
    assert_eq!(classify("pista@h: Permission denied (publickey).\r\n"), RemoteFailure::Auth);
    assert_eq!(classify("Host key verification failed.\r\n"), RemoteFailure::HostKey);
    assert_eq!(classify("ssh: connect to host 1.2.3.4 port 22: Connection refused"), RemoteFailure::Unreachable);
    assert_eq!(classify("boom"), RemoteFailure::Setup);
}
#[test]
fn targets_only_appear_after_the_double_dash() {
    assert_eq!(command_args("prod", PROBE), ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "--", "prod", PROBE]);
    for command in [PROBE, UPLOAD, SETUP, SETUP_REPLACE] {
        assert!(!command.contains(['$', '\'', '"', '`', '{']), "{command}");
    }
}
#[test]
fn fixed_commands_work_in_every_available_shell() {
    // sh, bash, zsh, fish 중 PATH에 있는 셸마다, 임시 HOME으로:
    //   `<shell> -c UPLOAD` (stdin "bin") → HOME/.local/bin/westerm 내용 "bin", 권한 0o755
    //   `<shell> -c PROBE` → 성공, 첫 줄 단어 두 개
    // 없는 셸은 건너뛴다. 최소한 sh는 반드시 돈다.
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test --manifest-path native/Cargo.toml ssh` → FAIL
- [ ] **Step 3: 구현** — Interfaces 그대로.
- [ ] **Step 4: 통과 확인** — Run: `cargo test --manifest-path native/Cargo.toml` → PASS
- [ ] **Step 5: 체크포인트** — 제안: `git commit -m "Add the dashboard's ssh commands for remote westerm"`

---

### Task 7: Linux 실행 파일과 접속 기록

**Files:**
- Modify: `native/src/update.rs`
- Create: `native/src/server/remote/binary.rs`, `native/src/server/remote/records.rs`
- Test: 각 파일의 unit test

**Interfaces:**
- Produces:
  - `update.rs`: `pub fn download_release_binary(version: &str, triple: &str, dest: &Path) -> Result<(), String>` — 기준 URL은 `WESTERM_RELEASES_URL` 또는 `https://github.com/SteelCrab/westerm/releases`. 실제 일은 `fn download_from(base: &str, version: &str, triple: &str, dest: &Path) -> Result<(), String>`: `{base}/download/v{version}/westerm-{triple}.tar.gz`와 `.sha256`을 `curl`로 받고, SHA-256을 확인하고(틀리면 "… does not match its SHA-256 sum"), `tar -xf`로 풀고, `westerm-{triple}/westerm`을 `dest`로 옮긴다 (부모 폴더 생성, unix 0755). 기존 `WorkDir`, `curl`, `sha256_hex`를 쓴다.
  - `binary.rs`:
    - `pub fn linux_triple(arch: &str) -> Option<&'static str>` — `x86_64`/`amd64` → `x86_64-unknown-linux-musl`, `aarch64`/`arm64` → `aarch64-unknown-linux-musl`.
    - `#[derive(Debug, PartialEq)] pub enum InstallPlan { UpToDate, Install, RemoteNewer }`, `pub fn install_plan(remote: Option<&str>, local: &str) -> InstallPlan` — 같으면 `UpToDate`; `westerm X.Y.Z (…`에서 X.Y.Z가 `update::is_newer(X.Y.Z, update::CURRENT)`이면 `RemoteNewer`; 나머지 `Install`.
    - `#[derive(Debug, PartialEq)] pub enum BinarySource { Given(PathBuf), Cached(PathBuf), Download(PathBuf), DevBuild }`, `pub fn binary_source(env_dir: Option<&Path>, channel: update::Channel, cache: &Path, triple: &str) -> BinarySource` — env 폴더 있으면 `Given(<dir>/westerm-<triple>)`; `Channel::Source`면 `DevBuild`; `cache`(= `remote_dir/bin/<CURRENT>/<triple>/westerm`)가 파일이면 `Cached`, 아니면 `Download(cache)`.
    - `pub async fn obtain(state: &Path, triple: &str) -> Result<PathBuf, Failure>` — `WESTERM_REMOTE_BINARY_DIR`와 `update::channel()`로 `binary_source`. `Given`인데 파일이 없으면 `Failure(DevBinary, 경로)`. `DevBuild` → `Failure(DevBinary, "set WESTERM_REMOTE_BINARY_DIR")`. `Download` → `spawn_blocking(download_release_binary)`, 오류는 `Failure(Download, …)`.
  - `records.rs` (폴더는 `remote_dir(state)`):
    - `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)] pub struct Record { pub hostname: String, pub port: u16, pub fingerprint: String, pub version: String, #[serde(default)] pub declined: Option<String> }` — `version`: 준비할 때의 로컬 `version_line()`. `declined`: 사용자가 host 교체를 거절한 로컬 `version_line()`.
    - `pub fn file_name(target: &str) -> String` — `h-` + 대상 글자 (`[A-Za-z0-9.-]`는 그대로, 나머지 바이트는 `_` + 소문자 hex 2자리) + `.json`. 파일은 `hosts/` 아래.
    - `pub fn load(dir: &Path, target: &str) -> Option<Record>`, `pub fn save(dir: &Path, target: &str, record: &Record) -> io::Result<()>` (임시 파일 → rename, 0600).
    - `pub fn add_pending_close(dir: &Path, target: &str, session: &str) -> io::Result<()>`, `pub fn take_pending_closes(dir: &Path, target: &str) -> Vec<String>` — `pending-close.json` = `{ "<target>": ["<id>", …] }`, 중복 없음.

- [ ] **Step 1: 실패하는 테스트 쓰기**

```rust
// update.rs
#[test] fn downloads_and_checks_a_linux_release() {
    // 임시 fixture: westerm-x86_64-unknown-linux-musl/westerm ("#!/bin/sh\n")를 tar czf로 묶고 "<sum>  <name>"으로 .sha256
    // 배치: <fixture>/download/v9.9.9/…; download_from("file://<fixture>", "9.9.9", triple, dest) → dest 내용이 같음
}
#[test] fn refuses_a_release_with_a_wrong_sum() { /* 틀린 sum → Err에 "SHA-256" */ }
// binary.rs
#[test] fn maps_linux_architectures() {
    assert_eq!(linux_triple("amd64"), Some("x86_64-unknown-linux-musl"));
    assert_eq!(linux_triple("arm64"), Some("aarch64-unknown-linux-musl"));
    assert_eq!(linux_triple("armv7l"), None);
}
#[test] fn never_downgrades() {
    let local = crate::version_line();
    assert_eq!(install_plan(Some(&local), &local), InstallPlan::UpToDate);
    assert_eq!(install_plan(Some("westerm 99.0.0 (host rs-1)"), &local), InstallPlan::RemoteNewer);
    assert_eq!(install_plan(Some("westerm 0.0.1 (host rs-1)"), &local), InstallPlan::Install);
    assert_eq!(install_plan(None, &local), InstallPlan::Install);
    let same_version_other_build = format!("westerm {} (host rs-other)", crate::update::CURRENT);
    assert_eq!(install_plan(Some(&same_version_other_build), &local), InstallPlan::Install);
}
#[test] fn picks_where_the_binary_comes_from() { /* Given / DevBuild / Cached / Download 네 경우 */ }
// records.rs
#[test] fn file_names_are_safe_for_any_target() {
    assert_eq!(file_name("prod"), "h-prod.json");
    assert_eq!(file_name("pista@msi-linux-private-pista"), "h-pista_40msi-linux-private-pista.json");
    assert_eq!(file_name("u@[::1]"), "h-u_40_5b_3a_3a1_5d.json");
    assert_eq!(file_name(".."), "h-...json");
}
#[test] fn records_round_trip() { /* save → load 같음; 없는 대상 → None; unix 0600 */ }
#[test] fn pending_closes_are_taken_once() {
    // add(t1,a), add(t1,b), add(t1,a), add(t2,c) → take(t1) == [a,b] → take(t1) == [] → take(t2) == [c]
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test --manifest-path native/Cargo.toml binary records release` → FAIL
- [ ] **Step 3: 구현** — Interfaces 그대로.
- [ ] **Step 4: 통과 확인** — Run: `cargo test --manifest-path native/Cargo.toml` → PASS
- [ ] **Step 5: 체크포인트** — 제안: `git commit -m "Find Linux builds for remote servers and remember remote connections"`

---

### Task 8: 대시보드를 거치는 원격 패인 (RemoteManager, relay, API)

**Files:**
- Create: `native/src/server/remote/quic.rs`, `native/src/server/remote/tunnel.rs` (`#[cfg(unix)]`), `native/src/server/remote/manager.rs`
- Modify: `native/src/server/remote/mod.rs`, `native/src/server/mod.rs`
- Test: `native/src/server/remote/manager.rs` (unit), `native/tests/remote.rs` (대시보드 종단 테스트)

**Interfaces:**
- Consumes: Task 1 (`version_line`, `PROTOCOL`, `HOST_BUILD`), Task 2 (`RemoteStage`, `RemoteFailure`, `ServerMessage::Remote`, `CLOSE_REMOTE_FAILED`, `for_remote_host`), Task 3 (`Identity`, `client_config`, `HANDSHAKE_TIMEOUT`, `remote_dir`, `SetupReport`, `HostInfo`, `parse_report`), Task 6 (`ssh::*`, `Failure`), Task 7 (`binary::*`, `records::*`).
- Produces:
  - `pub trait HostIo: AsyncRead + AsyncWrite + Unpin + Send {}` (모든 해당 타입에 구현), `pub type HostStream = Box<dyn HostIo>`.
  - `quic.rs`: `pub struct QuicLink`, `QuicLink::connect(identity: &Identity, hostname: &str, port: u16, fingerprint: &str, timeout: Duration) -> Result<QuicLink, Failure>` — 이름 풀이 첫 주소, 그 주소 계열에 맞게 `0.0.0.0:0` 또는 `[::]:0`에 새 endpoint, SNI `westerm`. `open(&self) -> io::Result<HostStream>` (`open_bi` → `tokio::io::join(recv, send)`), `is_alive(&self) -> bool` (`close_reason().is_none()`).
  - `tunnel.rs`: `pub struct Tunnel`, `Tunnel::start(target: &str, local: PathBuf, remote: &str) -> Result<Tunnel, Failure>` — `ssh -N -T -o BatchMode=yes -o ExitOnForwardFailure=yes -o StreamLocalBindUnlink=yes -o ServerAliveInterval=10 -L <local>:<remote> -- <target>`, `kill_on_drop(true)`, 로컬 소켓이 생길 때까지 최대 10초 (먼저 끝나면 stderr로 `Failure(classify, …)`). `open(&self) -> io::Result<HostStream>`, `is_alive(&mut self) -> bool`. 로컬 소켓 경로 `remote_dir/t-<sha256(target) 앞 8자>.sock`.
  - `manager.rs`:
    - `#[derive(Debug, PartialEq)] pub enum HostPlan { Use, Replace, Ask { sessions: usize }, Incompatible }`
    - `pub fn host_plan(host: &HostInfo, local_build: &str, declined: bool) -> HostPlan` — 빌드 같음 → `Use`; `sessions == 0` → `Replace`; `declined` → `protocol == PROTOCOL`이면 `Use` 아니면 `Incompatible`; 나머지 `Ask { sessions }`.
    - `pub struct RemoteManager` (서버별 `Target`: `link: tokio::sync::Mutex<Option<Link>>`, `stages: watch::Sender<Option<RemoteStage>>`, `decision: std::sync::Mutex<Option<oneshot::Sender<bool>>>`, `stale: AtomicBool`; `enum Link { Quic(QuicLink), #[cfg(unix)] Tunnel(Tunnel) }`)
    - `RemoteManager::new(state_dir: PathBuf) -> Self`; 클라이언트 신원은 처음 쓸 때 `load_or_create(remote_dir, "client")`.
    - `pub fn stages(&self, target: &str) -> watch::Receiver<Option<RemoteStage>>`
    - `pub async fn open_stream(&self, target: &str) -> Result<HostStream, Failure>`
    - `pub async fn invalidate(&self, target: &str)` — link를 버리고 `stale = true` (다음 한 번은 빠른 길을 건너뜀).
    - `pub fn decide(&self, target: &str, replace: bool) -> bool` — 기다리는 질문이 없으면 false.
    - `pub async fn forget_session(&self, target: &str, session: &str)` — `add_pending_close`; link가 살아 있으면 바로 전달.
    - `pub async fn update_host(&self, target: &str) -> Result<(), Failure>` — `SETUP_REPLACE` 실행, 기록의 `declined` 지움, link 버림.
- `open_stream` 순서 (서버별 `link` 잠금 안에서; 그래서 같은 서버의 패인들은 준비를 한 번만 한다):
  1. link가 살아 있으면 `open()`. 실패하면 link를 버리고 계속.
  2. 빠른 길: `!stale`이고 기록이 있고 `record.version == version_line()`이면 → `Connecting` → `QuicLink::connect(…, 2 s)`. 성공하면 link 저장, 끊긴 동안 닫은 세션 전달, `open()`.
  3. `Checking` → `ssh::run(PROBE)` → `parse_probe`. `os != "Linux"`나 `linux_triple`이 없으면 `Failure(Unsupported, "<os> <arch>")`.
  4. `install_plan`: `RemoteNewer` → `Failure(NewerRemote, 원격 버전 줄)`; `Install` → `Installing { version: CURRENT }` → `binary::obtain` → `ssh::run(UPLOAD, 파일 바이트)` → `PROBE`를 다시 실행해서 버전 줄이 `version_line()`과 같은지 확인 (다르면 `Failure(Setup, "the uploaded westerm reports <줄>")`).
  5. `Starting` → `ssh::run(SETUP, 지문 + "\n")` → `parse_report` (없으면 `Failure(Setup, stdout 끝부분)`).
  6. `host_plan(report.host, HOST_BUILD, record.declined == Some(version_line()))`: `Replace` → `SETUP_REPLACE`로 다시 보고받기; `Ask` → `Confirm { sessions }`을 알리고 `decision`을 기다림 → true면 `SETUP_REPLACE`, false면 `declined` 저장 후 protocol이 같으면 계속, 다르면 `Failure(Incompatible, …)`; `Incompatible` → 같은 실패.
  7. `ssh::resolve` → 기록 저장 (`hostname`, `report.port`, `report.fingerprint`, `version_line()`, `declined`).
  8. `Connecting` → 전송 방식: `WESTERM_REMOTE_TRANSPORT`가 `tunnel`이거나 `proxied`면 터널만; `quic`이면 QUIC만; 그 외는 QUIC(`HANDSHAKE_TIMEOUT`) 후 실패하면 터널. 터널은 unix에서만; Windows는 `Failure(Transport, "QUIC failed; the SSH tunnel fallback needs macOS or Linux")`. 둘 다 실패 → `Failure(Transport, …)`.
  9. `Connected { via, kill_on_logout: report.kill_on_logout }` → `stale = false` → 끊긴 동안 닫은 세션 전달 (task) → `open()`.
  - 끊긴 동안 닫은 세션 전달: id마다 스트림을 열고 `ws://host/session?session=<id>&kind=shell&cols=80&rows=24&width=640&height=408&lang=en`에 붙은 뒤 `{"type":"close"}`를 보내고 닫는다. 실패한 id는 다시 `add_pending_close`.
- `server/mod.rs`:
  - `AppState`에 `remote: RemoteManager`.
  - `session()`: `request.kind == SessionKind::Remote`면 로컬 `host.ensure()`를 부르지 않고 `remote_relay`로 업그레이드.
  - 지금의 `relay` 본문을 `async fn pipe<S: AsyncRead + AsyncWrite + Unpin>(to_browser: SplitSink<WebSocket, Message>, from_browser: SplitStream<WebSocket>, host: WebSocketStream<S>)`로 옮기고, 로컬과 원격이 같이 쓴다.
  - `async fn remote_relay(browser: WebSocket, request: AttachRequest, state: Shared)`:
    1. `stages(target)`를 구독하고 `open_stream(target)`을 기다리는 동안 `select!`로: 단계가 바뀌면 `ServerMessage::Remote(stage)` 텍스트를 보낸다; 브라우저의 `{"type":"close"}` → `forget_session` 후 1000으로 닫고 끝; `resize` 텍스트는 마지막 것만 보관; 입력은 버림; 브라우저가 닫히면 끝 (`open_stream` future를 버리면 결정 대기도 풀린다).
    2. `open_stream`이 끝나면, 마지막으로 보낸 단계와 지금 단계가 다를 때 한 번 더 보낸다.
    3. `Err(f)` → `Remote(Failed { reason, detail })`을 보내고, `f.reason.is_permanent()`면 `CLOSE_REMOTE_FAILED`, 아니면 1011로 닫는다.
    4. `Ok(stream)` → `request.for_remote_host().to_query()`로 WebSocket 핸드셰이크 (5초 제한). 실패나 시간 초과 → `invalidate(target)`, 1011로 닫음. 성공 → 보관한 `resize`를 먼저 보내고 `pipe`.
  - 새 API (모두 `origin_allowed` + JSON 검사, 아니면 400; `target`은 `normalize_ssh_target`, `session`은 `is_id`):
    - `POST /api/remote/decision` `{target, replace: bool}` → `decide`가 true면 204, false면 409.
    - `POST /api/remote/close` `{target, session}` → `forget_session` → 204.
    - `POST /api/remote/update-host` `{target}` → 성공 204, 실패 502 + `{"reason": …, "detail": …}`.

- [ ] **Step 1: 실패하는 테스트 쓰기**

```rust
// manager.rs unit
#[test]
fn host_plans_follow_the_users_choice() {
    let host = |build: &str, protocol, sessions| HostInfo { build: build.into(), protocol, sessions };
    assert_eq!(host_plan(&host("rs-a", 1, 3), "rs-a", false), HostPlan::Use);
    assert_eq!(host_plan(&host("rs-old", 1, 0), "rs-a", false), HostPlan::Replace);
    assert_eq!(host_plan(&host("rs-old", 1, 2), "rs-a", false), HostPlan::Ask { sessions: 2 });
    assert_eq!(host_plan(&host("rs-old", PROTOCOL, 2), "rs-a", true), HostPlan::Use);
    assert_eq!(host_plan(&host("rs-old", PROTOCOL + 1, 2), "rs-a", true), HostPlan::Incompatible);
}
```

`native/tests/remote.rs`에 가짜 원격 도우미 `FakeRemote`:
- 짧은 임시 루트(`/tmp/wr…`) 아래 `bin/ssh`, `rbin/uname`(항상 `Linux x86_64` 출력), `bins/westerm-x86_64-unknown-linux-musl`(= `BIN` 복사본), `home/`, `rstate/`, `lstate/`, `ssh.log`.
- `bin/ssh` (POSIX sh):

```sh
#!/bin/sh
# Fake ssh for tests: -G prints a config; otherwise the last argument runs here as the "remote" user.
for last; do :; done
case " $* " in *" -G "*) printf 'hostname 127.0.0.1\nport 22\nproxyjump none\n'; exit 0 ;; esac
printf '%s\n' "$last" >> "$FAKE_ROOT/ssh.log"
[ -n "$FAKE_SSH_DELAY" ] && sleep "$FAKE_SSH_DELAY"
HOME="$FAKE_ROOT/home" PATH="$FAKE_ROOT/rbin:$PATH" WESTERM_STATE_DIR="$FAKE_ROOT/rstate" \
  WESTERM_GATE_PORT=0 SHELL=/bin/sh exec sh -c "$last"
```

- 대시보드 실행: `BIN serve --port <p>`, env `PATH=<root>/bin:$PATH`, `HOME=<root>/lhome`, `WESTERM_STATE_DIR=<root>/lstate`, `WESTERM_REMOTE_BINARY_DIR=<root>/bins`, `FAKE_ROOT=<root>`, `SHELL=/bin/sh` (+ 필요하면 `FAKE_SSH_DELAY`). Drop 때 `rstate`와 `lstate`에 `BIN stop`.
- `open_pane(port, session) -> (ws, stages: Vec<Value>, ready: Value)` — `Origin: http://127.0.0.1:<p>` 헤더를 붙여 `/ws/session?session=<id>&kind=remote&target=fake&cols=80&rows=24&width=640&height=408&lang=en`에 연결하고, 크기를 보내고, `ready`가 올 때까지 `type == "remote"` 메시지를 모은다.

```rust
#[tokio::test]
async fn a_remote_pane_installs_westerm_and_survives_a_reconnect() {
    // stages에 {"stage":"connected","via":"quic"}가 있고 "failed"는 없음; ready.mode == "new"
    // <root>/home/.local/bin/westerm 이 있음; ssh.log에서 UPLOAD 줄 == 1
    // MARK 입력 → MARK_42; ws를 그냥 닫고 같은 session으로 다시 열기 → ready.mode == "reattached", 화면에 MARK_42
}
#[tokio::test]
async fn two_panes_opening_at_once_install_once() {
    // FAKE_SSH_DELAY=1; 서로 다른 session 두 개를 동시에 열기 → 둘 다 ready
    // ssh.log: PROBE 줄 == 1, UPLOAD 줄 == 1
}
#[tokio::test]
async fn closing_a_pane_while_connecting_ends_it_on_the_server() {
    // FAKE_SSH_DELAY=2; session "sclose"를 열고 첫 단계 메시지를 받자마자 {"type":"close"} → 소켓이 닫힘
    // session "skeep"을 열어 ready까지
    // rstate/host.sock의 /health가 10초 안에 sessions == 1; lstate/remote/pending-close.json에 "sclose" 없음
}
#[tokio::test]
async fn a_lost_gate_comes_back_and_the_shell_is_reattached() {
    // ready + MARK_42 뒤 rstate/gate.json의 pid에 SIGKILL; 브라우저 소켓을 닫고 같은 session으로 다시 열기
    // (1011 닫힘이면 최대 3번 다시 시도) → 15초 안에 ready.mode == "reattached", 화면에 MARK_42
}
#[tokio::test]
async fn remote_apis_check_the_origin() {
    // POST /api/remote/close Origin 없음 → 400; Origin과 올바른 JSON → 204
    // POST /api/remote/decision (기다리는 질문 없음) → 409
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test --manifest-path native/Cargo.toml host_plans` 와 `cargo test --manifest-path native/Cargo.toml --test remote` → FAIL
- [ ] **Step 3: 구현** — 위 Interfaces와 순서 그대로.
- [ ] **Step 4: 통과 확인** — Run: `cargo test --manifest-path native/Cargo.toml` → PASS (기존 integration.rs 포함), `cargo check --manifest-path native/Cargo.toml --target x86_64-pc-windows-msvc` → 성공
- [ ] **Step 5: 체크포인트** — 제안: `git commit -m "Open remote panes through the dashboard over QUIC or an SSH tunnel"`

---

### Task 9: 원격 패인 UI

**Files:**
- Create: `src/remoteApi.ts`
- Modify: `src/kinds.tsx`, `src/i18n/messages.ts`, `src/workspace.ts`, `src/terminal/connection.ts`, `src/components/Pane.tsx`, `src/App.tsx`, `src/paletteCommands.tsx`, `src/styles.css`
- Test: `tests/remote.test.ts` (추가), `tests/i18n.test.ts` (그대로; 네 언어 키 비교)

**Interfaces:**
- Consumes: Task 2 TS 타입과 경로.
- Produces:
  - `src/remoteApi.ts`: `postRemote(path: string, body: Record<string, unknown>): Promise<boolean>` (JSON POST, `response.ok`), `shouldPostRemoteClose(kind: SessionKind, target: string | null, end: boolean, readyState: number): boolean` — `kind === "remote" && target && end && readyState !== WebSocket.OPEN && readyState !== WebSocket.CONNECTING` (상수는 1과 0으로 비교해서 node 테스트에서도 돌게).
  - `Session.remote: RemoteStage | null`; `WorkspaceAction`에 `{ type: "remoteStage"; id: string; stage: RemoteStage }`.
  - `ConnectionEvent`에 `{ type: "remote"; stage: RemoteStage }`, `{ type: "remoteFailed"; reason: RemoteFailure; detail: string }`.
  - 문구 (`en` 기준, 아래 `ko`도 그대로; `ja`, `zh`는 번역하고 확실하지 않은 번역은 체크포인트 메모에 적는다):

| 키 | en | ko |
|---|---|---|
| `kinds.remote` | `{ name: "westerm", label: "Remote westerm", hint: "Install westerm on a server over SSH and connect over QUIC", paletteTitle: "Connect with remote westerm…" }` | `{ name: "westerm", label: "원격 westerm", hint: "SSH로 서버에 westerm을 설치하고 QUIC으로 연결합니다", paletteTitle: "원격 westerm 연결…" }` |
| `palette.remoteWesterm(target)` | `` `westerm ${target}` `` | 같음 |
| `palette.remoteWestermHint` | `Installs over SSH, connects over QUIC, survives disconnects` | `SSH로 설치, QUIC으로 연결, 끊겨도 유지` |
| `palette.updateRemoteHost(target)` | `` `Update the remote host · ${target}` `` | `` `원격 host 업데이트 · ${target}` `` |
| `remote.checking` | `Checking the server…` | `서버 확인 중…` |
| `remote.installing(version)` | `` `Installing westerm ${version}…` `` | `` `westerm ${version} 설치 중…` `` |
| `remote.starting` | `Starting westerm on the server…` | `서버에서 westerm 시작 중…` |
| `remote.connecting` | `Connecting…` | `연결 중…` |
| `remote.via.quic` / `remote.via.tunnel` | `QUIC` / `SSH tunnel` | `QUIC` / `SSH 터널` |
| `remote.confirm(sessions)` | `` `The server runs an older westerm host with ${sessions} open shells. Replacing it ends their programs; the shells come back with their saved screens.` `` | `` `서버의 westerm host가 이전 버전이고 열린 셸이 ${sessions}개 있어요. 바꾸면 그 안의 프로그램이 끝나고, 셸은 저장된 화면으로 돌아와요.` `` |
| `remote.replace` / `remote.keep` | `Replace now` / `Keep the old host` | `지금 바꾸기` / `이전 host 계속 쓰기` |
| `remote.killOnLogout` | `The server may end these shells at logout` | `로그아웃하면 서버가 셸을 끝낼 수 있어요` |
| `remote.failed.auth` | `SSH needs a key or ssh-agent; password logins are not supported` | `SSH 키나 ssh-agent가 필요해요. 비밀번호 로그인은 지원하지 않아요` |
| `remote.failed.hostKey` | `Unknown host key. Connect once with a plain SSH pane to accept it` | `처음 보는 host key예요. 일반 SSH 패인으로 한 번 접속해서 확인하세요` |
| `remote.failed.unreachable` | `Cannot reach the server over SSH` | `SSH로 서버에 닿지 않아요` |
| `remote.failed.unsupported` | `Only Linux on x86_64 or aarch64 is supported` | `x86_64 또는 aarch64 Linux만 지원해요` |
| `remote.failed.download` | `Could not download westerm for the server` | `서버용 westerm을 내려받지 못했어요` |
| `remote.failed.devBinary` | `Source build: set WESTERM_REMOTE_BINARY_DIR to a folder with Linux builds` | `소스 빌드예요. WESTERM_REMOTE_BINARY_DIR에 Linux 빌드 폴더를 지정하세요` |
| `remote.failed.newerRemote` | `The server has a newer westerm. Update westerm here first` | `서버의 westerm이 더 새 버전이에요. 이 컴퓨터의 westerm을 먼저 업데이트하세요` |
| `remote.failed.incompatible` | `The old remote host cannot talk to this westerm. Use "Update the remote host"` | `이전 원격 host는 이 westerm과 대화할 수 없어요. "원격 host 업데이트"를 쓰세요` |
| `remote.failed.transport` | `Neither QUIC nor an SSH tunnel could connect` | `QUIC과 SSH 터널 모두 연결하지 못했어요` |
| `remote.failed.setup` | `Could not start westerm on the server` | `서버에서 westerm을 시작하지 못했어요` |

- 동작:
  - `workspace.ts`: `TITLE_PREFIX.remote = "Remote"`; `open`에서 `remote` + target → 제목 `` `westerm ${target}` ``; `newSession`은 `remote: null`; `remoteStage` → `remote` 갱신; `restart` → `remote: null`; `disconnected`에서 `kind === "remote"`면 횟수와 상관없이 `{ status: "disconnected", notice: "reconnecting" }`.
  - `connection.ts`: `receive()`에서 `type === "remote"`: 실패 단계이고 `isPermanentFailure(reason)`면 `finish({ type: "remoteFailed", reason, detail })`, 아니면 `onEvent({ type: "remote", stage })` (`stage`는 `type`을 뺀 객체). `close()`: `shouldPostRemoteClose(...)`면 `postRemote(REMOTE_CLOSE_PATH, { target, session })`.
  - `Pane.tsx`: `remote` → `remoteStage` dispatch; `remoteFailed` → `status: "error"`, `detail: t.remote.failed[reason] + (detail ? \` (${detail})\` : "")`. 원격 패인의 상태 줄: 연결 전에는 단계 문구, 연결 후에는 기존 `pid N` 뒤에 `· ${t.remote.via[via]}`, `killOnLogout`이면 `· ${t.remote.killOnLogout}`. `remote.stage === "confirm"`이면 `.pane-overlay`(`role="alertdialog"`)에 `t.remote.confirm(sessions)`와 버튼 두 개 → `postRemote(REMOTE_DECISION_PATH, { target: session.target, replace })`.
  - `kinds.tsx`: `remote: { icon: ServerCog, imageLimit: 32 * MiB }`, `LAUNCHERS = ["shell", "browser", "editor", "ssh", "remote"]`.
  - `App.tsx`: `open()`에서 `ssh` 또는 `remote`이고 target이 없으면 SSH 목록 팔레트; `counts`에 `remote: 0`.
  - `paletteCommands.tsx`: `sshCommands`의 서버마다 `ssh` 다음에 `{ id: \`remote-${target}\`, title: t.palette.remoteWesterm(target), hint: t.palette.remoteWestermHint, group: t.palette.groups.remoteShell, icon: <KindIcon kind="remote" size={14} />, keywords: \`westerm remote quic persistent ${target}\`, run: () => open("remote", null, target) }`. `buildCommands`는 열린 원격 패인의 대상마다 `{ id: \`remote-update-${target}\`, title: t.palette.updateRemoteHost(target), group: g.remoteShell, keywords: \`westerm remote host update replace ${target}\`, run: () => void postRemote(REMOTE_UPDATE_HOST_PATH, { target }) }`.

- [ ] **Step 1: 실패하는 테스트 쓰기**

```ts
test("remote panes get a westerm title", () => {
  const state = workspaceReducer(createWorkspace("t1"), { type: "open", id: "sr1", kind: "remote", target: "prod" });
  assert.equal(state.sessions[0].title, "westerm prod");
  assert.equal(state.sessions[0].remote, null);
});
test("remote panes keep reconnecting; shells still give up", () => {
  let remote = workspaceReducer(createWorkspace("t1"), { type: "open", id: "sr1", kind: "remote", target: "prod" });
  for (let i = 0; i < 20; i++) remote = workspaceReducer(remote, { type: "disconnected", id: "sr1" });
  assert.equal(remote.sessions[0].status, "disconnected");
  let shell = workspaceReducer(createWorkspace("t1"), { type: "open", id: "ss1", kind: "shell" });
  for (let i = 0; i <= MAX_RECONNECTS; i++) shell = workspaceReducer(shell, { type: "disconnected", id: "ss1" });
  assert.equal(shell.sessions[0].status, "error");
});
test("remote stages are kept and cleared on restart", () => {
  // remoteStage connected(quic) → remote.stage === "connected"; restart → remote === null
});
test("the ssh picker offers westerm for each host", () => {
  const calls: unknown[][] = [];
  const commands = sshCommands([{ alias: "prod", hostname: null, user: null }], null, (...args) => calls.push(args), MESSAGES.en);
  commands.find((command) => command.id === "remote-prod")!.run();
  assert.deepEqual(calls, [["remote", null, "prod"]]);
});
test("remote panes report a close the socket cannot carry", () => {
  assert.equal(shouldPostRemoteClose("remote", "prod", true, 3), true);   // CLOSED
  assert.equal(shouldPostRemoteClose("remote", "prod", true, 1), false);  // OPEN
  assert.equal(shouldPostRemoteClose("remote", "prod", false, 3), false); // only detaching
  assert.equal(shouldPostRemoteClose("shell", null, true, 3), false);
});
```

(`MESSAGES`는 `src/i18n/messages.ts`의 `export const MESSAGES: Record<Locale, Messages>`.)

- [ ] **Step 2: 실패 확인** — Run: `node --import tsx --test tests/remote.test.ts` → FAIL
- [ ] **Step 3: 구현** — 위 동작 그대로. 확인 상자와 via 표시에 필요한 CSS는 `src/styles.css`의 기존 `.pane-overlay`, `.pane-detail` 규칙을 다시 쓰고, 새 규칙은 최소로.
- [ ] **Step 4: 통과 확인** — Run: `npm test && npm run type-check` → 모두 PASS
- [ ] **Step 5: 체크포인트** — 제안: `git commit -m "Show remote westerm panes, their stages, and the host replacement prompt"`

---

### Task 10: 문서와 변경 기록

**Files:**
- Modify: `docs/architecture.md`, `docs/sessions.md`, `docs/usage.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `CONTRIBUTING.ko.md`

**내용 (빠짐없이):**
- `architecture.md`: 구성도에 원격 gate와 터널; 경로 표에 `native/src/remote/`, `native/src/server/remote/`; Security 절: gate가 UDP를 받는다는 새 노출, TLS 1.3 상호 인증과 지문 고정, SSH로만 등록, QUIC Retry, WebSocket 세션 업그레이드만 통과; Known gap에 "로컬 프로그램이 4317을 통해 원격 셸도 열 수 있다".
- `sessions.md`: 원격 세션이 살아남는 경우(네트워크 전환, 잠자기, 대시보드 재시작, 서버 재부팅 후 "restored")와 host 교체 질문.
- `usage.md`: 원격 westerm 열기 (팔레트 → 서버 → `westerm <서버>`), 조건(Linux x86_64/aarch64, 키 또는 ssh-agent, `known_hosts`), 방화벽 UDP 4317–4326, 상태 표시(QUIC / SSH 터널), 오류 문구와 해결, 클라이언트 해제(`~/.local/state/westerm/remote/authorized_clients`에서 줄 삭제), linger(`loginctl enable-linger`), 환경 변수 `WESTERM_REMOTE_TRANSPORT`(`quic`|`tunnel`), `WESTERM_GATE_PORT`, `WESTERM_RELEASES_URL`, `westerm stop`이 gate도 멈춤.
- `CONTRIBUTING.md`/`.ko.md`: 개발 빌드로 원격 시험하기 — `brew install zig`, `cargo install cargo-zigbuild`, `cargo zigbuild --release --target x86_64-unknown-linux-musl --manifest-path native/Cargo.toml`, 파일을 `westerm-x86_64-unknown-linux-musl` 이름으로 한 폴더에 두고 `WESTERM_REMOTE_BINARY_DIR`로 지정.
- `CHANGELOG.md`: `## [Unreleased]` 절에 "Remote westerm: install over SSH, connect over QUIC" 항목 (절 안의 기존 소제목 형식을 따름).

- [ ] **Step 1: 문서 쓰기**
- [ ] **Step 2: 확인** — Run: `npm test && npm run type-check && npm run build` → 세 명령 모두 성공
- [ ] **Step 3: 체크포인트** — 제안: `git commit -m "Document remote westerm"`

---

### Task 11: 테스트 서버에서 종단 확인

**대상:** `pista@msi-linux-private-pista` (Ubuntu 26.04.1, x86_64, zsh, ufw 꺼짐, Linger=yes). 서버 쪽 westerm 설치는 사용자가 허락했다.

- [ ] **Step 1: 도구 설치** (사용자 허락) — Run: `brew install zig && cargo install cargo-zigbuild` → `cargo zigbuild --version` 출력
- [ ] **Step 2: 빌드** — Run: `npx vite build && cargo build --release --manifest-path native/Cargo.toml && cargo zigbuild --release --target x86_64-unknown-linux-musl --manifest-path native/Cargo.toml` → 두 실행 파일 생성
- [ ] **Step 3: 준비** — Run: `mkdir -p /tmp/wrb && cp native/target/x86_64-unknown-linux-musl/release/westerm /tmp/wrb/westerm-x86_64-unknown-linux-musl`
- [ ] **Step 4: 대시보드 실행 (백그라운드)** — Run: `WESTERM_STATE_DIR=/tmp/wq WESTERM_REMOTE_BINARY_DIR=/tmp/wrb ./native/target/release/westerm --port 4318`
- [ ] **Step 5: 처음 연결** — 브라우저(Claude in Chrome)로 http://127.0.0.1:4318 → 팔레트 → SSH 목록 → `westerm msi-linux-private-pista` → 단계 문구가 지나가고 `QUIC` 표시 → `uname -a`가 Linux를 출력. 밝은/어두운 테마 스크린샷. 서버 확인: `ssh pista@msi-linux-private-pista '~/.local/bin/westerm --version; cat ~/.local/state/westerm/gate.json'` → 로컬 `--version`과 같은 버전 줄, 포트 4317.
- [ ] **Step 6: 시나리오**
  - a. 로컬 대시보드 재시작 → 같은 셸 ("reconnected").
  - b. 서버의 gate를 `kill` → 패인이 다시 연결 → 같은 셸.
  - c. `WESTERM_REMOTE_TRANSPORT=tunnel`로 대시보드 재시작 → `SSH 터널` 표시 → 셸 동작.
  - d. 원격에서 `sleep 600` 실행 → 서버에서 `~/.local/bin/westerm stop` → 패인 재연결 → "restored".
  - e. host 교체 질문: `native/src/host/store.rs`의 주석 한 줄을 바꾸고 로컬과 Linux를 다시 빌드 → 원격에서 `sleep 600` 실행 중에 대시보드 재시작 → 확인 상자 → "이전 host 계속 쓰기" → 같은 셸, `sleep` 계속 → 팔레트 "원격 host 업데이트" → "restored". 끝나면 주석을 되돌리고 다시 빌드.
  - f. (선택, 사용자 도움) Wi‑Fi를 10초 이내로 껐다 켬 → 같은 셸.
- [ ] **Step 7: 정리** — 로컬 시험 대시보드를 멈춘다(`WESTERM_STATE_DIR=/tmp/wq ./native/target/release/westerm stop`). 서버의 westerm은 그대로 둔다 (요청한 설치). 서버에 남은 것(실행 파일, host, gate, 포트)을 사용자에게 보고한다.
