# 원격 westerm: SSH로 설치하고 QUIC으로 연결하기

- 날짜: 2026-11-05
- 상태: 초안 (승인 전)
- 브랜치: `feat/remote-westerm` (워크트리 `.claude/worktrees/remote-quic`, 기준 커밋 `07e2684` = `origin/main`)

표시 규칙:

- **(사용자)**: 사용자가 정한 것.
- **(제 판단)**: 제가 정한 것. 사용자에게 제안했고 반대가 없었지만, 명시적으로 승인받지는 않았어요.
- **(빌더)**: 구현하는 사람이 정해도 되는 것.

## 1. 의도와 이유

- 지금 SSH 패인은 로컬 PTY host 안에서 `ssh`를 실행해요.
- 노트북이 잠들거나 Wi‑Fi가 바뀌면 TCP 연결이 끊겨요. 그러면 `ssh`가 끝나고, 원격 셸과 그 안의 작업도 끝나요.
- westerm의 핵심 약속은 "세션이 살아남는다"예요. 지금은 이 약속이 로컬 셸에만 적용돼요.
- 이 기능은 이 약속을 원격 셸까지 넓혀요.
  - SSH로 원격 서버에 westerm을 설치하고 실행해요. **(사용자)**
  - 원격 셸은 원격 서버의 westerm host가 붙잡아요. **(사용자)**
  - 로컬 westerm은 QUIC으로 원격 host와 대화해요. **(사용자)**
  - SSH는 준비 단계(설치, 실행, 키 교환)에만 써요. mosh와 같은 방식이에요. **(사용자)**
- QUIC을 고른 이유는 "빠르고, 안전하고, 신뢰도가 좋다"예요. **(사용자)** 이 기능에서 각각 이런 뜻이에요.
  - 빠름: 연결 준비가 짧아요. 패인마다 스트림이 따로라서, 한 패인에 출력이 많아도 다른 패인이 기다리지 않아요.
  - 안전: TLS 1.3이 프로토콜에 들어 있어요.
  - 신뢰: 클라이언트 IP가 바뀌어도 연결을 이어 가요(connection migration).

## 2. 목표

- G1. 명령 팔레트에서 원격 westerm 패인을 열 수 있어요. 서버는 지금 SSH 목록(`~/.ssh/config`)에서 고르거나 `user@host`로 입력해요.
- G2. 원격에 westerm이 없거나 로컬과 빌드가 다르면 자동으로 설치해요.
- G3. 원격 패인의 입력과 출력은 QUIC으로 오가요.
- G4. 네트워크가 바뀌거나 끊겨도 원격 셸은 원격에서 계속 돌아요. 다시 연결되면 화면이 돌아와요 ("reattached").
- G5. 원격 서버가 재부팅돼도, 다음 연결 때 셸이 저장된 화면과 폴더로 돌아와요 ("revived").
- G6. SSH로 로그인할 수 있는 사용자만 QUIC 연결을 열 수 있어요. **(사용자)**
- G7. UDP가 막힌 네트워크에서도 원격 패인을 쓸 수 있어요. 이때는 SSH 터널을 대신 써요. **(제 판단)**
- G8. 원격 패인과 로컬 패인을 한 탭에 섞어 쓸 수 있어요. 레이아웃 저장과 복원도 로컬 패인과 같아요. **(사용자)**

## 3. 범위 밖 (non-goals)

- 원격 에디터(terminal-code)와 원격 브라우저(terminal-browser)를 원격 westerm으로 실행하는 것. 기존 "원격 브라우저" 명령은 그대로 둬요. **(제 판단)**
- Linux가 아닌 원격 서버(macOS, Windows). **(사용자: 원격은 Linux)**
- 비밀번호나 OTP처럼 사람이 입력해야 하는 SSH 인증. 키 또는 ssh-agent만 지원해요. **(제 판단)**
- 원격 westerm의 웹 UI를 다른 기기에서 여는 것.
- 원격 서버의 방화벽을 자동으로 여는 것 (root 권한이 필요해요).
- 원격 host를 systemd 서비스로 등록해서 재부팅 직후 바로 시작하는 것. 다음 연결 때 시작해도 G5를 만족해요.

## 4. 하면 안 되는 것 (anti-goals)

아래 중 하나라도 생기면, 기능이 동작해도 실패로 봐요.

- 원격에 인증 없이 쓸 수 있는 포트가 생기는 것. gate는 등록된 클라이언트 인증서만 받아야 해요.
- 원격 명령 문자열에 사용자 입력이나 변하는 값이 들어가는 것. `ssh`는 원격 명령을 원격 셸 문자열로 실행하기 때문이에요. 원격 명령은 고정 문자열이고, 데이터는 stdin으로만 보내요.
- 기존 SSH 패인의 동작이 바뀌는 것.
- gate 코드가 바뀔 때마다 로컬 셸이 재시작되는 것. gate 코드는 `HOST_SOURCES`에 넣지 않아요.
- 체크섬을 확인하지 않은 실행 파일을 원격에 올리는 것.
- 연결이 끊겼는데 "연결됨"처럼 보이는 것. 상태는 정직하게 보여 줘요.
- 원격 사용자의 설정 파일(`.bashrc`, `.profile`, `PATH`)을 고치는 것.
- 원격이 끊긴 동안 닫은 패인의 셸이 원격에 영원히 남는 것.

## 5. 제약

- 원격: Linux x86_64와 aarch64. 릴리스에 있는 musl 정적 실행 파일을 써요. 배포판과 상관없이 돌아요.
- 로컬: macOS, Linux, Windows에서 빌드와 테스트가 통과해야 해요 (CI가 세 OS에서 돌아요).
- QUIC은 TLS 1.3이 필수예요. 그래서 실행 파일에 TLS 라이브러리가 처음으로 들어가요. 지금 `update.rs`는 TLS 라이브러리를 피하려고 시스템 `curl`을 써요. 다운로드는 계속 `curl`로 하고, QUIC에만 TLS 라이브러리를 써요. 실행 파일이 커지는 것은 받아들여요.
- 대시보드는 계속 `127.0.0.1`에서만 들어요.
- UI 문구는 `src/i18n/messages.ts`에 네 언어(en, ko, ja, zh-CN)로 넣어요. 팔레트 `keywords`는 영어로 둬요.
- `shared/protocol.ts`와 `native/src/protocol.rs`는 함께 바꾸고, 양쪽에서 검증해요.
- `protocol.rs`는 `HOST_SOURCES`에 있어요. 새 세션 종류를 넣으면 host 빌드 id가 한 번 바뀌어요. 업데이트 후 로컬 셸이 한 번 "restored"로 다시 시작해요. 이것은 받아들여요.
- Unix 소켓 경로는 104바이트 이하여야 해요 (SSH 터널의 로컬 소켓).
- 문서: `docs/architecture.md`, `docs/sessions.md`, `docs/usage.md`, `CHANGELOG.md`를 함께 고쳐요. 레포 문서는 영어예요.

## 6. 구조

```
브라우저 ─ WebSocket ─ 로컬 대시보드 (127.0.0.1:4317)
                        ├─ Unix 소켓 ──────────────── 로컬 host ─ 로컬 셸
                        └─ 원격 연결 관리자
                             ├─ QUIC (UDP) ─ 원격 gate ─ Unix 소켓 ─ 원격 host ─ 원격 셸
                             └─ (대체) ssh -L 터널 ─────── Unix 소켓 ─ 원격 host
                             (준비: ssh로 설치, remote-setup 실행, 키 교환)
```

- **원격 host**: 지금의 `westerm host` 그대로예요. 바꾸지 않아요.
- **원격 gate** (`westerm gate`, 새 명령): QUIC 서버예요. 인증된 스트림을 원격 host의 Unix 소켓에 그대로 이어요. 바이트를 해석하지 않고 넘겨요. 단, 첫 요청 줄은 검사해요 (8.4).
- **원격 remote-setup** (`westerm remote-setup`, 새 명령): SSH로 실행하는 짧은 명령이에요. host와 gate를 준비하고, 접속 정보를 JSON 한 줄로 출력하고 끝나요.
- **로컬 원격 연결 관리자** (새 모듈, 예: `native/src/remote/`): 서버마다 연결 하나를 관리해요. 준비, 설치, QUIC 연결, SSH 터널 대체, 재연결을 맡아요.
- 대시보드와 host 사이의 대화 방식(HTTP/1.1 + WebSocket)은 그대로예요. 지금은 Unix 소켓 위에서 하고, 원격은 QUIC 양방향 스트림 위에서 해요. 그래서 `server/mod.rs`의 relay 코드를 그대로 다시 써요. **(사용자 승인: "같은 대화 방식을 원격 host에도 그대로")**

## 7. 연결 순서

원격 패인을 열거나 다시 연결할 때:

1. 브라우저가 대시보드에 붙어요: `/ws/session?kind=remote&target=<대상>&session=<id>&...`
2. 대시보드는 그 대상의 연결 상태를 봐요. 살아 있는 연결이 있으면 7번으로 가요.
3. 저장된 접속 정보(8.6)가 있으면, SSH 없이 QUIC을 바로 시도해요. 성공하면 7번으로 가요.
4. SSH로 서버를 확인해요: CPU 종류(`uname -m`)와 설치된 westerm의 `--version` 줄.
5. westerm이 없거나 `--version` 줄이 로컬과 다르면 설치해요 (8.1).
6. SSH로 `remote-setup`을 실행해요. stdin으로 로컬 클라이언트 인증서 지문을 보내고, stdout에서 JSON을 받아요 (8.3). 그다음 QUIC으로 연결해요. QUIC이 안 되면 SSH 터널을 써요 (8.5).
7. 연결 위에 새 스트림을 열고, 원격 host의 `/session`으로 WebSocket 업그레이드를 해요. 이때 세션 종류는 `shell`이고, `target`은 보내지 않아요. 그다음은 지금의 relay와 같아요.

- 3번에서 1–2초 안에 핸드셰이크가 안 되면 4번으로 넘어가요. gate가 멈췄거나, 포트나 인증서가 바뀌었을 수 있어요. **(빌더: 정확한 시간)**
- SSH 명령은 모두 `ssh -T -o BatchMode=yes -o ConnectTimeout=10 -- <대상> '<고정 문자열>'` 형태예요. 대상은 `normalize_ssh_target`으로 검증해요.
- 원격 명령은 원격 사용자의 로그인 셸이 실행해요. 이 셸은 bash가 아닐 수 있어요 (테스트 서버는 zsh). 고정 문자열은 sh, bash, zsh, fish에서 같은 뜻이어야 해요. 복잡한 문법(리다이렉션 조합, 함수, 따옴표 중첩)은 피해요.
- 4–6번은 SSH 연결 하나로 묶어도 돼요. **(빌더)**

## 8. 세부 설계

### 8.1 설치 **(제 판단)**

- 설치 위치: 원격의 `~/.local/bin/westerm`. `install.sh`와 같은 위치예요.
  - 원격 상태 폴더도 기본 위치(`~/.local/state/westerm`)를 그대로 써요. 사용자가 서버에서 westerm을 직접 쓰고 있으면, 그 host를 함께 써요. 상태 폴더 하나에 host 하나라는 규칙은 그대로예요. **(제 판단, 검토 후)**
  - 다운그레이드는 하지 않아요. 원격 westerm의 버전이 로컬보다 높으면 설치하지 않고 "로컬 westerm을 업데이트하세요"를 보여 줘요. **(제 판단, 검토 후)**
  - 서로 다른 버전의 로컬 두 대가 같은 서버를 쓰면, 높은 버전이 이기고 낮은 버전은 위 안내를 받아요. 문서에 제약으로 적어요.
- 빌드 일치 규칙: 원격 `westerm --version` 줄이 로컬과 정확히 같아야 해요. 이 줄에는 버전과 host 빌드 id가 들어 있어요.
  - `native/build.rs`는 해시하기 전에 줄 끝(CRLF → LF)을 통일해요. 그렇지 않으면 Windows 릴리스(CRLF 체크아웃)의 host 빌드 id가 Linux 빌드와 달라서, Windows 로컬은 영원히 일치하지 않아요. 이 변경으로 모든 플랫폼의 host 빌드 id가 한 번 바뀌어요. **(제 판단, 검토 후)**
- 실행 파일 준비 (로컬에서):
  - 릴리스 빌드(설치본, macOS 앱): GitHub 릴리스 `v<버전>`에서 `westerm-<arch>-unknown-linux-musl.tar.gz`와 `.sha256`을 받아요. 체크섬을 확인하고 풀어요. `update.rs`의 `curl`, `tar`, SHA-256 코드를 다시 써요.
  - 받은 파일은 로컬 상태 폴더에 캐시해요: `remote/bin/<버전>/<arch>/westerm`.
  - 소스 빌드(개발용): 같은 버전의 릴리스에는 이 기능이 없어요. 그래서 환경 변수로 Linux 실행 파일을 지정해요 (예: `WESTERM_REMOTE_BINARY_DIR` 안의 `westerm-x86_64-unknown-linux-musl`). 지정이 없으면 만드는 방법을 오류로 알려 줘요. **(빌더: 이름과 형식)**
- 올리기: SSH stdin으로 실행 파일을 보내요. 원격 명령은 고정 문자열이에요:
  `mkdir -p ~/.local/bin && cat > ~/.local/bin/.westerm.upload && chmod 755 ~/.local/bin/.westerm.upload && mv -f ~/.local/bin/.westerm.upload ~/.local/bin/westerm`
  - 이름 바꾸기로 교체하므로, 돌고 있는 원격 westerm은 영향을 받지 않아요.
  - 올린 뒤 `--version`을 다시 확인해요.
- 원격 host 교체 규칙 **(사용자)**:
  - 새 실행 파일의 host 빌드 id가 돌고 있는 원격 host와 다를 때 적용해요. 로컬 westerm을 업데이트한 뒤가 대표적인 경우예요.
  - 원격 host에 실행 중인 셸이 없으면 바로 교체해요.
  - 실행 중인 셸이 있으면 교체 전에 사용자에게 물어봐요. 교체하면 원격에서 돌던 프로그램이 끝나고, 셸은 저장된 화면으로 "revived"가 된다고 알려 줘요.
  - 사용자가 거절하면 이전 host를 계속 써요. 단, 통신 규칙(8.3의 `protocol`)이 같을 때만이에요. 다르면 연결할 수 없다고 알리고, 교체만 고를 수 있게 해요.
  - 답은 (대상, 새 빌드) 한 쌍마다 한 번만 물어요. 로컬 빌드가 다시 바뀌거나, 사용자가 팔레트의 "원격 host 업데이트" 명령을 쓰면 다시 물어요. **(제 판단)**
  - gate는 셸을 붙잡지 않으므로 항상 새 빌드로 바로 교체해요. 이전 host를 계속 쓸 때도 새 gate가 바이트만 넘기므로 문제가 없어요.
- 설치 중에는 패인에 진행 상태를 보여 줘요 (8.7).

### 8.2 인증서와 인증 **(제 판단)**

- 로컬: 클라이언트 키와 자체 서명 인증서를 한 번 만들어요. 위치는 로컬 상태 폴더의 `remote/client.key`, `remote/client.crt`예요 (0600).
- 원격: gate가 서버 키와 자체 서명 인증서를 한 번 만들어요. 위치는 원격 상태 폴더의 `remote/gate.key`, `remote/gate.crt`예요 (0600).
- 지문 = 인증서 DER의 SHA-256.
- 서버 확인: 로컬은 `remote-setup`이 SSH로 알려 준 gate 지문과 같은 인증서만 믿어요. SSH가 이미 `known_hosts`로 서버를 확인했기 때문에, 이 지문은 믿을 수 있어요.
- 클라이언트 확인: gate는 원격의 `remote/authorized_clients`에 있는 지문의 인증서만 받아요. 이 파일은 `remote-setup`만 고쳐요. 즉 SSH로 로그인한 사람만 고칠 수 있어요.
- gate는 핸드셰이크마다 `authorized_clients`를 다시 읽어요. 줄을 지우면 그 클라이언트는 다음 연결부터 막혀요.
- TLS 1.3 상호 인증, ALPN은 `westerm/1`.
- 처음 보는 서버라 `known_hosts`에 없으면, BatchMode 때문에 SSH가 실패해요. 그러면 "일반 SSH 패인으로 한 번 접속해서 host key를 확인하세요"라고 알려 줘요.

### 8.3 `westerm remote-setup` **(제 판단)**

- stdin 한 줄: 로컬 클라이언트 인증서 지문 (hex 64자). 형식이 틀리면 거부해요.
- 지문이 `authorized_clients`에 없으면 추가해요.
- 원격 host가 돌고 있지 않으면 시작해요. 지금 `host_client.rs`의 시작 코드를 다시 써요 (`setsid`로 분리 실행).
- 다른 빌드의 host가 돌고 있으면, 스스로 교체하지 않아요. 그 host의 빌드, `protocol`, 실행 중인 셸 수를 JSON으로 알려 줘요. 교체는 로컬이 8.1 규칙으로 정하고, 고정 옵션(예: `remote-setup --replace-host`)으로 다시 실행해서 해요.
- 이를 위해 host의 `/health` 응답에 `protocol`(정수)을 더해요. `protocol`은 대시보드와 host 사이 통신 규칙(attach 질의, JSON 메시지)이 바뀔 때만 올려요. 이 변경은 host 코드 변경이에요.
- gate가 이 빌드로 돌고 있는지 확인하고, 아니면 이전 gate를 멈추고 새로 시작해요 (`setsid`).
- stdout에 JSON 한 줄을 쓰고 끝나요. 예:
  `{"v":1,"version":"westerm 0.2.0 (host rs-…)","port":4317,"fingerprint":"<hex>","socket":"/home/pista/.local/state/westerm/host.sock","host":{"build":"rs-…","protocol":1,"sessions":3}}`
  - `socket`은 SSH 터널(8.5)에 써요.
  - `host`는 지금 돌고 있는 host의 정보예요. 로컬은 이것으로 교체 여부를 정해요.
- systemd-logind의 `KillUserProcesses=yes`인 서버에서는 SSH 로그아웃 때 host와 gate가 죽어요. `remote-setup`은 이것을 감지하면 경고를 JSON에 넣고, 패인은 그 경고를 보여 줘요. **(빌더: 감지 방법)**

### 8.4 `westerm gate` **(제 판단)**

- QUIC 서버. 기본 포트는 UDP 4317이에요.
  - 같은 서버의 다른 사용자가 이미 쓰고 있으면 다음 포트를 차례로 시도해요 (예: 4317–4326). 실제 포트는 `remote-setup` JSON으로 알려 줘요. **(빌더: 범위)**
  - 환경 변수나 옵션으로 포트를 바꿀 수 있어요. **(빌더: 이름)**
- 상태 폴더 하나에 gate 하나만 돌아요 (`gate.lock`). 로그는 `gate.log`예요.
- 받은 양방향 스트림마다 원격 host의 Unix 소켓에 연결하고, 양쪽 바이트를 그대로 이어요.
- 첫 요청은 허용 목록으로 검사해요. WebSocket 세션 업그레이드(`GET /session?… HTTP/1.1` + `Upgrade: websocket`)만 통과해요. 나머지(예: `POST /shutdown`, `GET /health`)는 거부해요.
- host가 `101`로 답한 뒤에만 양쪽 바이트를 이어요. 그래서 업그레이드에 실패한 요청 뒤에 붙인 다른 요청은 host에 닿지 않아요. **(제 판단, 계획 중 보완)**
- 연결이 끊겨도 gate는 계속 돌아요.
- `westerm stop`은 host와 함께 gate도 멈춰요. **(빌더: 방법)**
- gate는 Unix 전용이어도 돼요. Windows에서는 빌드는 되지만 명령은 "지원 안 함"으로 끝나요.

### 8.5 QUIC 연결과 대체 경로 **(제 판단)**

- 라이브러리: `quinn` + `rustls`. 인증서 생성은 `rcgen`. 암호 백엔드는 musl과 Windows에서 빌드가 쉬운 `ring`을 권해요. **(빌더: 최종 선택과 버전)**
- QUIC 주소: `ssh -G <대상>`의 `hostname`을 로컬에서 이름 풀이한 주소 + JSON의 `port`. (원격의 `$SSH_CONNECTION`은 NAT 뒤에서 틀리므로 쓰지 않아요.)
- 서버 하나에 QUIC 연결 하나. 패인마다 스트림 하나.
- keep-alive는 몇 초 간격, idle timeout은 수십 초예요. Wi‑Fi 전환처럼 짧은 끊김은 같은 연결로 넘어가요. 더 긴 끊김(잠자기)은 새 연결로 다시 붙어요. 원격 셸은 그동안 계속 돌아요. **(빌더: 값)**
- 클라이언트 IP가 바뀌면 QUIC connection migration으로 연결을 옮겨요. 보내기가 계속 실패하면 새 UDP 소켓으로 다시 묶어요(rebind). **(빌더: 감지 방법)**
- SSH 터널 대체 (G7):
  - 언제: QUIC 핸드셰이크가 몇 초 안에 안 될 때. 또는 대상에 `ProxyJump`나 `ProxyCommand`가 있어서 직접 UDP가 닿지 않을 때.
  - 방법: `ssh -N -T -o BatchMode=yes -o ExitOnForwardFailure=yes -o StreamLocalBindUnlink=yes -o ServerAliveInterval=10 -L <로컬 소켓>:<원격 socket> -- <대상>`
  - 대시보드는 로컬 소켓에 연결해요. 그다음은 로컬 host와 똑같아요.
  - 터널 프로세스가 끝나면 다시 시작해요.
  - 다음 재연결 때는 QUIC을 먼저 다시 시도해요.
  - 로컬이 Windows이면 이번 범위에서 터널 대체를 지원하지 않아요. QUIC이 안 되면 이유를 보여 줘요.

### 8.6 저장 파일 **(제 판단)**

로컬 상태 폴더:

| 경로 | 내용 |
|---|---|
| `remote/client.key`, `remote/client.crt` | 클라이언트 키와 인증서 (0600) |
| `remote/hosts/<대상>.json` | `hostname`, `port`, `fingerprint`, `version`. 대상 이름은 안전한 파일 이름으로 바꿔요. |
| `remote/bin/<버전>/<arch>/westerm` | 내려받은 Linux 실행 파일 캐시 |
| `remote/t-<짧은 id>.sock` | SSH 터널 소켓 (경로 104바이트 제한) |

원격 상태 폴더 (`~/.local/state/westerm`):

| 경로 | 내용 |
|---|---|
| `remote/gate.key`, `remote/gate.crt` | gate 키와 인증서 (0600) |
| `remote/authorized_clients` | 허용된 클라이언트 지문, 한 줄에 하나 (0600) |
| `gate.lock`, `gate.json`, `gate.log` | gate 잠금, 실제 포트와 빌드, 로그 |

### 8.7 화면 (UI) **(제 판단)**

- 새 세션 종류: `remote`. 저장 형식은 `ssh`처럼 `target`을 가져요. 저장 파일 검증(`shared/workspace-file.ts`)도 `ssh`와 같은 규칙이에요.
- 이름: 기존 SSH 팔레트 그룹 이름이 이미 "원격 셸"이에요. 그래서 새 종류는 다른 이름을 써요. 예: 영어 "Remote westerm", 한국어 "원격 westerm". **(빌더: 최종 문구, 아이콘)**
- 팔레트: SSH 호스트 목록에서 서버마다 명령을 하나 더 보여 줘요. 예: `westerm <대상>`, 설명 "설치 후 QUIC으로 연결, 끊겨도 유지". 활동 막대에도 실행 버튼을 하나 추가해요. **(빌더: 배치)**
- 기존 `ssh` 패인은 그대로예요.
- 패인 상태 줄에 단계를 보여 줘요: 서버 확인 중, westerm 설치 중, 연결 중, 연결됨(QUIC), 연결됨(SSH 터널), 재연결 중.
  - 방법: 대시보드가 `ready` 전에 새 JSON 메시지(예: `{"type":"remote","stage":…}`)를 보내요. 터미널 출력 바이트에 섞지 않아요. 섞으면 흐름 제어용 ack 수가 틀어지기 때문이에요. **(빌더: 메시지 이름과 모양)**
- 재연결: 지금은 8번 실패하면 멈춰요(`MAX_RECONNECTS`). 원격 패인은 횟수 제한 없이 최대 8초 간격으로 계속 시도하고, "재연결 중"을 보여 줘요. 수동 재시작은 그대로예요.
- 오류 문구는 원인과 해결 방법을 함께 보여 줘요:
  - SSH 키 인증 실패 → 키나 ssh-agent가 필요하다고 알려 줘요.
  - `known_hosts`에 없음 → 일반 SSH로 한 번 접속하라고 알려 줘요.
  - Linux가 아님, 지원하지 않는 CPU → 지원 범위를 알려 줘요.
  - 다운로드 실패, 체크섬 불일치, 개발 빌드인데 실행 파일 지정 없음.
  - QUIC과 SSH 터널이 모두 실패.
- 원격 host 교체 질문(8.1)은 그 대상의 원격 패인 안에 보여 줘요. 버튼은 "교체"와 "이전 host 계속 쓰기" 두 개예요. 같은 대상의 다른 패인들은 답을 기다려요. **(빌더: 모양)**
- 팔레트에 "원격 host 업데이트: <대상>" 명령을 둬요. 이전 host를 계속 쓰기로 했던 것을 나중에 교체할 때 써요.
- 원격이 끊긴 동안 사용자가 원격 패인을 닫으면, 대시보드가 그 세션 id를 기억했다가 다음 연결 때 원격에서도 끝내요 (anti-goal 참고). **(빌더: 저장 방법)**
- 원격 패인에서 "새 에디터를 이 폴더에서 열기"는 적용하지 않아요. 에디터는 로컬 홈 폴더에서 열려요.

### 8.8 보안

- 새로 생기는 노출: 원격 gate가 네트워크에서 UDP를 받아요. 지금까지 westerm은 어떤 포트도 외부에 열지 않았어요. 대응:
  - TLS 1.3 상호 인증과 인증서 고정. 등록되지 않은 클라이언트는 핸드셰이크에서 끝나요.
  - 클라이언트 등록은 SSH를 거쳐서만 돼요.
  - QUIC의 주소 검증(Retry)으로 증폭 공격을 줄여요.
  - 요청 줄 허용 목록 (8.4).
- 사용자 입력은 원격 명령 문자열에 넣지 않아요 (anti-goal). 대상은 `normalize_ssh_target`으로 검증하고 `--` 뒤에 둬요.
- 올리는 실행 파일은 릴리스 SHA-256으로 확인해요.
- 클라이언트 해제: 원격 `authorized_clients`에서 줄을 지워요. 문서에 적어요.
- 남는 위험(문서화): 로컬 127.0.0.1:4317은 지금처럼 로컬 프로그램이 접근할 수 있어요. 이제 로컬 프로그램이 원격 셸도 열 수 있어요. 기존 "Known gap"에 이 내용을 더해요.
- `docs/architecture.md`의 Security 절에 위 내용을 적어요.

## 9. 테스트

- 단위 테스트(Rust):
  - 지문 검증기: 맞는 인증서는 통과, 다른 인증서는 거부.
  - `authorized_clients` 읽기, 추가, 중복 방지.
  - `remote-setup` JSON 읽기와 잘못된 입력 거부.
  - 대상 이름 → 파일 이름 변환.
  - 빌드 일치 규칙, Linux arch → 릴리스 파일 이름.
  - gate 요청 허용 목록 (WebSocket 세션 업그레이드만).
- 통합 테스트(`native/tests/`, 실제 실행 파일):
  - 임시 상태 폴더로 host와 gate를 띄우고 127.0.0.1 UDP로 연결해요. SSH 없이 `remote-setup`을 로컬에서 직접 실행해서 준비 과정을 흉내 내요.
  - QUIC으로 셸에 붙고, 연결을 끊고, 다시 붙어서 화면이 돌아오는지 확인해요.
  - 등록되지 않은 클라이언트 인증서는 거부돼요.
  - WebSocket 세션 업그레이드가 아닌 요청은 거부돼요.
  - Windows CI에서는 gate 테스트를 건너뛰어요.
- UI 테스트(`node:test`): `remote` 종류 검증, 저장 파일 검증, 팔레트 명령.
- 수동 종단 테스트: 테스트 서버 `pista@msi-linux-private-pista` **(사용자)**
  - 서버 정보 (2026-11-05 확인): Ubuntu 26.04.1, x86_64, 로그인 셸 zsh, 같은 LAN(192.168.219.105). westerm과 Rust는 없어요. ufw는 꺼져 있어요. `Linger=yes`라서 로그아웃해도 host와 gate가 살아 있어요.
  - 테스트용 Linux 실행 파일은 이 Mac에서 zig + cargo-zigbuild로 교차 빌드해요 (`x86_64-unknown-linux-musl`). **(사용자: zig 설치 허용)**
  - 처음 설치, 연결, 셸 사용.
  - 로컬 대시보드 재시작 후 다시 붙기.
  - 네트워크 전환(또는 로컬 UDP 소켓 강제 교체) 후 같은 셸 유지.
  - 원격 재부팅 후 "revived".
  - UDP 차단(잘못된 포트 등) 때 SSH 터널로 전환.

## 10. 문서

- `docs/architecture.md`: 구성 요소 그림, 경로 표, Security 절, Known gap.
- `docs/sessions.md`: 원격 세션이 살아남는 경우와 아닌 경우.
- `docs/usage.md`: 원격 westerm 열기, 방화벽(UDP 4317), 문제 해결, 클라이언트 해제.
- `CHANGELOG.md`: 새 기능.

## 11. 빌더에게 맡기는 것

- 크레이트 버전, 모듈 나누기, 메시지와 함수 이름, 아이콘과 문구.
- 시간 값(핸드셰이크 제한, keep-alive, idle timeout), 포트 범위.
- 개발 빌드용 실행 파일 지정 방식.
- 4–6번 SSH 단계를 연결 하나로 묶을지 여부.
- 원격이 끊긴 동안 닫은 세션을 기억하는 방법.

## 12. 의존 관계와 위험

- 메인 체크아웃에서 0.1.1 작업이 진행 중이에요 (여러 창이 셸을 공유, `CLOSE_ENDED`). `host/session.rs`, `host/registry.rs`, `protocol.rs`, `connection.ts`, `Pane.tsx`를 고치고 있어요. 아직 커밋되지 않았어요.
  - 이 브랜치는 `origin/main`에서 만들어요. 0.1.1이 `main`에 들어가면 이 브랜치를 그 위로 옮겨요(rebase). **(제 판단, 검토 후)**
  - 원격 host는 로컬 host와 같은 코드예요. 그래서 옮긴 뒤에는 원격 패인도 0.1.1의 동작(여러 창 공유, 닫힘 코드)을 그대로 따라요. 따로 만들 것은 없어요.
  - 이 기능은 0.1.1 다음 릴리스에 들어가요. 정확한 버전 번호는 릴리스 때 정해요.
- 테스트 서버는 같은 LAN에 있을 때만 닿아요. 처음 확인 때는 Mac이 다른 네트워크에 있어서 닿지 않았어요.
- `ring`은 C 코드가 있어서 교차 빌드에 C 컴파일러가 필요해요. zig가 이 역할을 해요. CI는 Linux 러너에서 직접 빌드하므로 문제없어요.
- QUIC은 UDP가 막힌 곳(일부 회사망, 클라우드 기본 방화벽)에서 안 돼요. G7의 SSH 터널이 이 위험을 줄여요.
