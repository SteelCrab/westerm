# 에이전트 사이드바: 워크스페이스의 실행 폴더별 AI 에이전트

- 날짜: 2026-11-07
- 상태: 초안 (승인 전)
- 브랜치: `feat/agent-sidebar` (워크트리 `.claude/worktrees/feat-agent-sidebar`, 기준 커밋 `5bd569a` = `origin/main`)
- 앞선 기능: PR #21 `feat/agent-sessions` (에이전트 감지, AI 표시, 정보 카드, 재부팅 후 이어서 시작). 이 문서는 그 위에 사이드바를 더해요.
- 목업: https://claude.ai/artifact/WpXwdV9J9sLi8APVFruLX5 의 "A4 · 워크스페이스 폴더별"

표시 규칙:

- **(사용자)**: 사용자가 정한 것. 대화와 목업에서 확인했어요.
- **(제 판단)**: 제가 정한 것. 사용자가 반대할 수 있어요.
- **(빌더)**: 구현하는 사람이 정해도 되는 것.

## 1. 의도와 이유

- 사용자는 westerm 셸에서 Claude Code, Codex, Gemini CLI를 여러 개 돌려요. 어느 에이전트가 어디서 무엇을 하는지 한곳에서 보고 싶어 해요. **(사용자)**
- Gemini CLI 감지는 지금 없어요(`agent.rs`는 Claude Code와 Codex만 알아요). 이번에 같이 넣어요 (10절). **(사용자: "이번에 같이 넣기")**
- 보는 단위는 "지금 보고 있는 워크스페이스"이고, 묶는 기준은 "세션이 실행된 폴더"예요. **(사용자)**
- 각 에이전트는 그 AI의 실제 아이콘으로 보여요. Codex는 OpenAI 로고예요. **(사용자)**
- 폴더마다 끝난 대화("지난 대화")를 보여 주고, 이어서 시작할 수 있어요. **(사용자)**
- 사이드바의 모양은 westerm 기존 사이드바 문법(40px 헤더, 28px 행, 7px 상태 점, 트리 안내선)을 따라요. **(사용자: 첫 A안의 대시보드형 디자인을 거절)**

## 2. 목표

- G1. 활동 바에 "에이전트" 버튼이 생겨요. 누르면 사이드바 자리에 에이전트 패널이 열려요. **(사용자)**
  - 위쪽 레이아웃: 버튼이 패널을 열고 닫아요. **(제 판단)**
  - 사이드바 레이아웃: 버튼이 사이드바의 내용을 "세션 목록 ↔ 에이전트 패널"로 바꿔요. 기존 사이드바 스위치는 그대로 사이드바를 열고 닫아요. **(제 판단)**
  - 좁은 화면(640px 이하): 서랍으로 열려요. 기존 서랍 규칙(바깥 탭, Esc, 고르면 닫힘)을 따라요. **(제 판단)**
  - 패널이 열려 있는지는 이 브라우저에 기억해요 (`localStorage`의 `westerm.agentsView`). **(제 판단)**
  - 어느 워크스페이스에서든 에이전트가 입력을 기다리면 버튼에 노란 점이 보여요. **(사용자: 목업)**
- G2. 패널 맨 위는 워크스페이스 메뉴예요. 기존 `WorkspaceMenu`를 그대로 써요. 고른 워크스페이스가 화면의 워크스페이스예요. 패널은 따로 범위를 갖지 않아요. **(제 판단)**
  - 다른 워크스페이스에 입력을 기다리는 에이전트가 있으면 메뉴 옆에 "다른 곳 N"이 보여요. **(사용자: 목업)**
- G3. 그 워크스페이스의 셸 세션을 실행 폴더별로 묶어요. **(사용자)**
  - 세션의 폴더: 에이전트가 돌면 에이전트 프로세스의 폴더(`agent.cwd`), 아니면 셸 프로세스의 폴더(새 `place.cwd`). **(제 판단)**
  - 셸만 묶어요. SSH, terminal-browser, terminal-code, 문서 패널은 빼요. **(제 판단)**
  - 폴더 순서: 워크스페이스의 탭 순서, 탭 안의 패널 순서에서 처음 나온 순서. 상태가 바뀌어도 줄이 움직이지 않아요. **(제 판단)**
  - 폴더를 모르는 셸(Windows, 아직 첫 보고 전)은 "알 수 없는 폴더" 묶음에 넣어요. 에이전트가 사라져 보이면 안 돼요. **(제 판단)**
- G4. 폴더 행은 접고 펼 수 있어요. 보여 주는 것: 폴더 이름(굵게)과 앞 경로(흐리게), git 브랜치, 셸 수, 에이전트 수, 입력을 기다리는 에이전트가 있으면 노란 점. 마우스를 올리면 전체 경로. **(사용자: 목업)**
  - 이름 규칙: 마지막 폴더 이름이 이름이에요. 앞 경로는 부모가 홈이면 `~/`, 아니면 `<부모 이름>/`. 홈 자체는 `~`. **(제 판단)**
  - 접힘 상태는 기억하지 않아요. **(제 판단)**
- G5. 에이전트 행: 왼쪽에 AI 아이콘, 이름(세션 이름 규칙 `sessionLabel` 그대로), 오른쪽에 상태 표시와 시간. **(사용자: 목업)**
  - 상태 표시: 입력 필요 = 노란 점이 깜빡임, 작업 중 = 파란 원이 돎, 완료(안 본 사이 끝남) = 초록 체크, 대기 = 회색 점. 기존 `agentDisplay` 규칙 그대로예요. 움직임 줄이기 설정에서는 움직이지 않아요. **(제 판단)**
  - 시간: 지금 상태가 된 뒤 흐른 시간(`agent.since`). 입력 필요는 노란 글자예요. **(제 판단)**
  - 입력 필요 행은 두 번째 줄에 "입력을 기다려요"를 보여 줘요. 질문 내용은 이번 범위가 아니에요 (3절). **(제 판단)**
  - 한 폴더 안 순서: 입력 필요, 완료, 작업 중, 대기. 같으면 패널 순서. **(제 판단)**
  - 행을 누르면 그 패널로 가요(기존 `focus` 동작). 포커스된 패널의 행은 워크스페이스 색으로 칠해요. **(제 판단)**
- G6. 에이전트가 없는 폴더는 "에이전트 없음"과 시작 버튼(Claude, Codex, Gemini 아이콘)을 보여 줘요. 누르면 그 폴더에서 새 셸 패널이 열리고 `claude`, `codex`, `gemini` 중 하나가 실행돼요. **(사용자: 목업)**
  - 에이전트가 있는 폴더도 폴더 행에 마우스를 올리면 같은 시작 버튼이 보여요. **(제 판단)**
- G7. 지난 대화: 폴더마다 그 폴더에서 끝난 Claude Code, Codex, Gemini CLI 대화를 최근 순으로 5개까지 보여 줘요. 각 줄: AI 아이콘, 제목, 언제, "이어서" 버튼. **(사용자)**
  - 지금 이 대시보드에서 돌고 있는 대화는 빼요 (모든 세션의 `agent.conversation`). **(제 판단)**
  - "이어서"를 누르면 그 폴더에서 새 셸 패널이 열리고 `claude --resume <id>`, `codex resume <id>`, `gemini --resume <id>` 중 하나가 실행돼요. **(사용자)**
  - 처음에는 펼쳐 있어요. **(사용자: "지난 대화를 남기고 다시 보여줘")**
  - 제목이 없으면 "제목 없는 대화"예요. **(제 판단)**
- G8. AI 아이콘은 실제 로고예요: Claude는 주황 별 모양, Codex는 OpenAI 로고(글자 색을 따라 밝은 테마에서 검정, 어두운 테마에서 흰색), Gemini는 색 있는 반짝임 모양. 출처는 `@lobehub/icons-static-svg@1.95.1`(MIT)의 `claude-color.svg`, `openai.svg`, `gemini-color.svg`예요. **(사용자)**
  - Gemini 로고의 그라디언트 id는 아이콘마다 달라야 해요(한 화면에 여러 개). **(제 판단)**
  - 기존 AI 표시(`AgentMark`: 패널 헤더, 세션 목록)의 별 아이콘도 같은 로고로 바꿔요. 상태 고리는 그대로예요. 한 화면에 두 가지 아이콘이 섞이면 안 돼요. **(제 판단)**
- G9. 원격 창에서도 같게 동작해요. 서버의 셸, 서버의 폴더, 서버의 지난 대화를 보여 줘요. **(제 판단: 문서 패널과 같은 방식)**

## 3. 범위 밖

- Claude Code, Codex, Gemini CLI 밖의 에이전트.
- 입력을 기다리는 질문의 내용, 허용/거부 버튼(목업 C안). **(제 판단)**
- 에이전트별 할 일, 바뀐 파일, 토큰 사용량(목업 B안). **(사용자: A안 선택)**
- 지난 대화의 검색, 삭제, 이름 바꾸기.
- 사이드바에서 에이전트를 멈추는 기능.

## 4. 실패로 보는 것 (anti-goals)

- 상태가 바뀔 때마다 줄이 위아래로 움직이는 것.
- 사이드바를 연 동안 디스크를 크게 계속 읽는 것. 지난 대화 읽기는 요청 한 번에 정해진 양만 읽어요 (6절).
- 브라우저가 보낸 문자열이 그대로 셸 명령이 되는 것. 명령은 호스트가 정해진 조각(에이전트 종류, 검사한 대화 id)으로만 만들어요.
- 기존 세션 목록, 탭, 패널 동작이 바뀌는 것.

## 5. 구조

```
PTY host (셸마다 1초 폴링, macOS·Linux)
  ├─ agent  : 기존. 에이전트 종류·상태·폴더·대화 id
  └─ place  : 새. 셸 프로세스 폴더 + git 브랜치. 바뀔 때만 보냄
        │ /ws/session 메시지 {"type":"place","place":{"cwd","branch"}|null}
        ▼
UI  Session.place ──► agentFolders(state, home) ──► AgentPanel
                                      ▲
Dashboard  GET /api/agents/history?dir=<절대 경로>  (지난 대화)
           원격 창: /remote/<대상>/api/agents/history → 서버 대시보드
```

- 새 세션 시작: `/ws/session` 쿼리에 `cwd=<절대 경로>`, `agent=claude|codex`, `conversation=<id>`(선택)를 더해요. 호스트는 **새로 만드는 셸에만** 이것을 써요. 셸을 그 폴더에서 시작하고, 프롬프트가 뜨면(기존 재개와 같은 규칙: 300ms 조용, 최대 5초) 명령을 입력해요. 다시 붙거나 복원되는 세션에서는 무시해요.
- 명령은 호스트가 만들어요: 대화 없이 `claude` / `codex` / `gemini`, 대화가 있으면 기존 `AgentRecord::resume_command`와 같은 모양(`claude --resume <id>`, `codex resume <id>`, `gemini --resume <id>`).
- git 브랜치: 폴더에서 위로 올라가며 `.git`을 찾아요. `.git`이 파일이면(워크트리) `gitdir:` 경로를 따라가요. `HEAD`가 `ref: refs/heads/<이름>`이면 이름, 분리된 HEAD면 커밋 앞 7자리, 아니면 없음. `git` 프로그램은 실행하지 않아요. **(제 판단)**

## 6. 지난 대화 읽기 규칙

- Claude Code: `<Claude 폴더>/projects/<인코딩한 경로>/<대화 id>.jsonl`. Claude 폴더는 기존 `claude_home()`(`CLAUDE_CONFIG_DIR`, 아니면 `~/.claude`). 인코딩: 경로의 영문자·숫자가 아닌 글자를 모두 `-`로 바꿔요 (이 컴퓨터에서 확인: `/Users/pista/westerm/.claude/worktrees/x` → `-Users-pista-westerm--claude-worktrees-x`).
  - 파일 수정 시각 순으로 최근 것부터 봐요. 이름이 대화 id 모양(16진수와 `-`, 8–64자)인 파일만 봐요.
  - 제목: `custom-title`(`customTitle`) > `ai-title`(`aiTitle`) > `summary`(`summary`) > `last-prompt`(`lastPrompt`). 같은 종류가 여러 번이면 마지막 것.
  - 인코딩이 겹칠 수 있어서(`/a/b-c`와 `/a/b/c`), 줄에 `cwd`가 있으면 첫 `cwd`가 요청한 폴더와 같아야 해요.
  - 파일 하나에서 16 MiB까지만 읽어요.
- Codex: `<Codex 폴더>/sessions/YYYY/MM/DD/rollout-*.jsonl`. Codex 폴더는 `CODEX_HOME`, 아니면 `~/.codex`.
  - 날짜 폴더와 파일 이름을 거꾸로(최근 것부터) 훑어요. 첫 줄(`session_meta`)의 `payload.cwd`가 요청한 폴더와 같고 `payload.id`가 대화 id 모양인 것만 써요.
  - 한 요청에서 최근 파일 500개까지만 봐요. 첫 줄은 1 MiB까지만 읽어요. 첫 줄은 바뀌지 않으므로 경로별로 메모리에 기억해요(최대 4096개).
  - 제목: `<Codex 폴더>/session_index.jsonl`의 `{"id","thread_name"}`. 같은 id가 여러 번이면 마지막 것. 파일 끝 4 MiB까지만 읽어요.
- Gemini CLI: Gemini 폴더는 `$GEMINI_CLI_HOME/.gemini`, 아니면 `~/.gemini` (Gemini CLI 0.64.0의 `homedir()` 규칙).
  - 프로젝트 이름: `<Gemini 폴더>/projects.json`의 `{"projects":{"<경로>":"<이름>"}}`에서 요청한 폴더의 이름. 없으면 `<Gemini 폴더>/tmp/*/.project_root` 파일 내용이 요청한 폴더와 같은 폴더(최대 256개만 봄).
  - 대화 파일: `<Gemini 폴더>/tmp/<이름>/chats/session-*.jsonl`과 `session-*.json`(옛 형식). 하위 폴더(서브에이전트)는 보지 않아요. 수정 시각 순.
  - `.jsonl`: 첫 줄의 `sessionId`(대화 id 모양)를 써요. 첫 줄의 `kind`가 `subagent`면 빼요. 제목은 `summary` 문자열이 있는 마지막 줄, 없으면 `type`이 `user`인 첫 줄의 글(`content`가 문자열이면 그것, 배열이면 `text`들을 이은 것).
  - `.json`: 객체 하나(`sessionId`, `summary`, `messages`)로 같은 규칙.
  - 파일 하나에서 16 MiB까지만 읽어요.
- 합치기: 세 목록을 수정 시각 최근 순으로 합쳐 5개. 응답: `{"conversations":[{"kind":"claude"|"codex"|"gemini","id","title"|null,"updated":<ms>}]}`.
- 제목은 한 줄로 만들고 앞뒤 공백을 지우고 120자까지 잘라요.
- `dir`은 절대 경로여야 해요. 아니면 400. 읽기 권한은 문서 경로와 같아요(`read_allowed`).

## 7. 보안

- `cwd`는 절대 경로, NUL 없음, 4096바이트 이하만 받아요. 호스트는 폴더가 없으면 기존 규칙(홈)으로 시작해요.
- `agent`는 `claude`, `codex`, `gemini`만, `conversation`은 대화 id 모양만 받아요. `conversation`만 오고 `agent`가 없으면 요청 전체를 거부해요.
- `agent`는 `kind=shell`에서만 받아요.
- 지난 대화 경로는 GET만 있어요. 같은 출처 검사(Host, `Sec-Fetch-Site`)는 문서 경로와 같아요.
- 지난 대화 응답에는 제목과 id만 있어요. 대화 내용은 보내지 않아요.

## 8. 화면 문구 (en 기준, 4개 언어)

- 활동 바 버튼: "Agents" / 힌트 "AI agents in this workspace, by folder"
- 패널 제목 줄: "Folders" + 수, "N agents"
- "Elsewhere N" (다른 워크스페이스의 입력 필요), 마우스 올림: "N agents need your input in other workspaces"
- "No agents", "Start here", 버튼 이름 "Start Claude Code here", "Start Codex here", "Start Gemini CLI here"
- 에이전트 이름(기존 `agent.names`에 더함): "Gemini CLI"
- "Unknown folder"
- "Needs your input"
- "Past conversations N", "Resume", 버튼 이름 "Resume <제목>", "Untitled conversation"
- 시간: 기존 `agent.card.ago`/`for` 문구를 다시 써요. **(빌더: 짧은 꼴이 필요하면 새 키)**
- 빈 패널: "No shells in this workspace"
- Windows: "Agents show on macOS and Linux"

## 9. 테스트

- Rust 단위: git 브랜치(일반, 워크트리 `.git` 파일, 분리된 HEAD, 저장소 밖), 지난 대화(Claude 인코딩, 제목 우선순위, `cwd` 다름 건너뜀, 개수와 순서; Codex 첫 줄 일치, 제목 색인, 개수; Gemini 프로젝트 이름 찾기, 서브에이전트 빼기, 제목), 요청 검사(`cwd`, `agent`, `conversation`), 시작 명령 모양, `place` 메시지 JSON, Gemini 찾기·옵션 유지·화면 상태.
- Rust 통합: `agent=claude&cwd=<임시 폴더>`로 새 셸을 열면 가짜 `claude`가 그 폴더에서 돌고 에이전트로 보여요. 같은 세션에 다시 붙으면 명령을 다시 입력하지 않아요. 임시 git 저장소에서 연 셸은 `place` 메시지에 브랜치를 보내요.
- TS: 폴더 묶기(순서, 알 수 없는 폴더, 셸만), 폴더 이름 규칙, 사이드바 보기 표(레이아웃 × 좁은 화면 × 버튼), 지난 대화에서 돌고 있는 대화 빼기, `place` 메시지 읽기, 새 세션의 쿼리, `ready` 뒤 시작 정보 지우기.
- 수동: 개발 서버(포트 4318, 따로 상태 폴더)에서 밝은·어두운 테마 스크린샷.

## 10. Gemini CLI 감지 (`native/src/host/agent.rs`)

Gemini CLI 0.64.0(`/opt/homebrew/lib/node_modules/@google/gemini-cli`)의 도움말과 코드에서 확인했어요 (2026-11-07).

- 찾기: 명령이 `gemini`이거나, node가 `@google/gemini-cli` 패키지의 스크립트(예: `…/@google/gemini-cli/bundle/gemini.js`)를 실행할 때. Claude, Codex와 같은 규칙(경로로 실행, npm 패키지)이에요.
- 에이전트가 아닌 것: `-p`/`--prompt`(헤드리스), `--acp`, `--experimental-acp`, `--list-sessions`, `--delete-session`, `-l`/`--list-extensions`, `-v`/`--version`, `-h`/`--help`, 하위 명령 `mcp`, `extensions`/`extension`, `skills`/`skill`, `hooks`/`hook`, `gemma`.
- 대화 id: 명령줄의 `-r`/`--resume <id>`나 `--session-id <id>`가 대화 id 모양일 때만. `latest`나 숫자(번호)는 id가 아니에요. Gemini는 프로세스별 파일을 쓰지 않아서, 그 밖에는 id를 몰라요.
- 상태: 화면만 봐요. 작업 중 = `esc to cancel`(Gemini의 진행 줄 `(esc to cancel, 3s)`). 입력 필요 = 기존 질문 목록에 `allow execution of`, `apply this change?`를 더해요(`Do you want to proceed?`는 이미 `do you want to`에 걸려요).
- 이어서 시작(재부팅 후): id가 있으면 `gemini --resume <id> <유지 옵션>`, 없으면 `gemini --resume latest <유지 옵션>`(그 폴더의 가장 최근 대화). Codex의 `--last`와 같은 한계예요.
- 유지 옵션: 값을 받는 `-m`, `--model`, `--approval-mode`, `--policy`, `--admin-policy`, `--allowed-mcp-server-names`, `--allowed-tools`, `-e`, `--extensions`, `--include-directories`; 값 없는 `-y`, `--yolo`, `-s`, `--sandbox`, `--skip-trust`, `--screen-reader`.
- 버리는 옵션: 시작 질문(`-i`/`--prompt-interactive`, 위치 인자), `-w`/`--worktree`(이어서 시작할 때 새 워크트리를 또 만들면 안 돼요), 재개 옵션(`-r`, `--resume`, `--session-file`, `--session-id`), 출력 옵션(`-o`, `--output-format`, `--raw-output`, `--accept-raw-output-risk`), `-d`/`--debug`.
- 정보 카드: Gemini는 버전과 대화 id를 몰라요. 카드의 대화 줄은 Codex처럼 "그 폴더의 최근 대화"예요.

## 11. 남은 결정 (빌더)

- 패널 CSS의 세부 값. 목업과 기존 사이드바 값을 따라요.
- 지난 대화를 다시 읽는 때: 패널이 열릴 때와 그 폴더의 돌던 대화가 끝날 때. 그 밖의 주기는 빌더가 정해요 (30초보다 자주는 안 돼요).
