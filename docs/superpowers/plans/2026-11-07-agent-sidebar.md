# 에이전트 사이드바 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 활동 바의 "에이전트" 버튼이 사이드바 자리에 패널을 연다. 패널은 화면의 워크스페이스 셸을 실행 폴더별로 묶고, 폴더마다 Claude Code·Codex·Gemini CLI 에이전트(실제 로고), git 브랜치, 지난 대화, 시작·이어서 버튼을 보여 준다.

**Architecture:** 호스트는 셸마다 1초 폴링에서 셸 폴더와 git 브랜치(`place`)를 바뀔 때만 `/ws/session`으로 보낸다. 호스트의 에이전트 감지는 Gemini CLI를 더 안다. 새 셸은 쿼리의 `cwd`, `agent`, `conversation`으로 그 폴더에서 시작하고, 호스트가 만든 명령을 프롬프트가 뜨면 입력한다(기존 재개 타이머 재사용). 대시보드의 `GET /api/agents/history`가 세 에이전트의 기록 파일에서 그 폴더의 지난 대화를 읽는다. 화면은 순수 함수 `agentFolders`로 묶고 `AgentPanel`이 그린다.

**Tech Stack:** Rust (tokio, axum, serde_json), TypeScript/React 19, lucide-react, node:test, cargo test.

**Spec:** `docs/superpowers/specs/2026-11-07-agent-sidebar-design.md` (Gemini 감지 규칙은 10절, 지난 대화 읽기는 6절)

## Global Constraints

- `shared/protocol.ts`와 `native/src/protocol.rs`는 같은 태스크에서 함께 바꾼다.
- 호스트가 쓰는 새 파일(`native/src/git.rs`)은 `native/build.rs`의 `HOST_SOURCES`에 넣는다.
- UI 문구는 `src/i18n/messages.ts`에 en, ko, ja, zh 네 언어로. 값이 들어가는 문구는 함수로. 컴포넌트에 문구를 직접 쓰지 않는다.
- 레포 문서(`docs/`, `CHANGELOG.md`)는 영어. 코드 주석도 영어(레포 관례).
- 명령 문자열은 호스트만 만든다. 브라우저는 `agent`(claude|codex|gemini)와 검사한 `conversation`만 보낸다.
- 커밋 금지: 에이전트는 `git commit`/`git push`를 실행하지 않는다. 각 태스크 끝에서 커밋 명령을 제안만 한다. 메시지는 레포 관례대로 명령형 영어 한 줄(`feat(scope): …`), `Co-Authored-By` 줄 없음.
- 개발 중 사용자의 westerm(포트 4317, `~/.local/state/westerm`)을 건드리지 않는다. 수동 실행은 `WESTERM_STATE_DIR=/tmp/wa ./native/target/release/westerm --port 4318`.
- 사용자의 실제 `~/.claude`, `~/.codex`, `~/.gemini`는 테스트에서 읽지 않는다. 테스트는 임시 폴더와 `CLAUDE_CONFIG_DIR`/`CODEX_HOME`/`GEMINI_CLI_HOME`, 또는 폴더를 인자로 받는 함수를 쓴다.
- 확인 명령: `npm test`, `npm run type-check`, `npm run build`, `cargo clippy --all-targets --manifest-path native/Cargo.toml -- -D warnings`. Rust 하나만: `cargo test --manifest-path native/Cargo.toml -- <이름>`. TS 하나만: `node --import tsx --test tests/<파일>.test.ts`.
- 새 워크트리에서 `cargo test`는 `dist/`가 있어야 컴파일된다(`npx vite build` 먼저). 이 워크트리에는 이미 있다.
- 작업 폴더: `/Users/pista/westerm-projects/westerm/.claude/worktrees/feat-agent-sidebar` (브랜치 `feat/agent-sidebar`).

## Review Focus

1. 다시 붙거나 복원되는 세션이 시작 명령을 또 입력하는 것: 같은 세션에 두 번째로 붙어도 명령은 한 번만. → Task 4 `a_launch_is_typed_once`, Task 6 `ready clears the launch`.
2. 상태가 바뀔 때 폴더 줄이 움직이는 것: 폴더 순서는 패널 순서로 고정. → Task 7 `folders keep pane order when states change`.
3. Claude 기록 폴더 이름이 겹치는 두 경로(`/a/b-c`, `/a/b/c`): 다른 폴더의 대화가 섞이면 안 됨. → Task 5 `claude_skips_conversations_of_another_folder`.
4. 기록 파일이 깨졌거나 아주 클 때: 오류 없이 건너뛰고 읽는 양에 한도. → Task 5 `broken_lines_and_files_are_skipped`, `codex_scan_stops_at_its_limit`.
5. Gemini의 `--worktree`나 시작 질문이 재부팅 뒤 다시 실행되는 것: 버림. → Task 3 `gemini_keeps_its_flags_and_drops_prompts_and_worktrees`.

## File Map

| 파일 | 책임 | 태스크 |
|---|---|---|
| `native/src/git.rs` (새) | 폴더의 git 브랜치 | 1 |
| `native/build.rs`, `native/src/main.rs` | `HOST_SOURCES`, `mod git` | 1 |
| `native/src/protocol.rs`, `shared/protocol.ts` | `Place`, `place` 메시지, `Launch`, 쿼리 `cwd`/`agent`/`conversation`, `is_conversation_id` 이동 | 2 |
| `native/src/protocol.rs`, `shared/protocol.ts`, `native/src/host/agent.rs`, `src/i18n/messages.ts`, `src/components/AgentCard.tsx`, `src/styles.css` | `AgentKind::Gemini`와 감지 | 3 |
| `native/src/host/agent.rs`, `session.rs`, `registry.rs`, `native/tests/integration.rs` | place 폴링·전송, 새 셸의 폴더와 시작 명령 | 4 |
| `native/src/server/agents.rs` (새), `server/mod.rs`, `docs.rs`, `remote_window.rs` | 지난 대화 API | 5 |
| `src/workspace.ts`, `src/terminal/connection.ts`, `TerminalView.tsx`, `src/components/Pane.tsx` | `Session.place`/`cwd`/`launch`, 메시지와 쿼리 | 6 |
| `src/agents.ts` (새) | 폴더 묶기, 이름 규칙, 사이드바 보기, 지난 대화 거르기, API | 7 |
| `src/agentIcons.tsx` (새), `src/components/AgentMark.tsx` | 실제 로고 | 8 |
| `src/components/AgentPanel.tsx` (새), `ActivityBar.tsx`, `App.tsx`, `src/useAgentsView.ts` (새), `src/styles.css`, `src/i18n/messages.ts` | 패널과 연결 | 9 |
| `docs/usage.md`, `docs/sessions.md`, `docs/architecture.md`, `CHANGELOG.md` | 문서 | 10 |

---

### Task 1: git 브랜치 읽기 (`native/src/git.rs`)

**Files:**
- Create: `native/src/git.rs`
- Modify: `native/src/main.rs` (`mod git;`), `native/build.rs` (`HOST_SOURCES`에 `"src/git.rs"`)

**Interfaces:**
- Produces: `pub fn branch(dir: &Path) -> Option<String>`

- [ ] **Step 1: 실패하는 테스트** (`git.rs` 안 `#[cfg(test)] mod tests`, `tempfile`)
  - `a_branch_from_head`: `<tmp>/.git/HEAD` = `ref: refs/heads/main\n`, `branch(<tmp>/a/b)` → `Some("main")` (하위 폴더에서 위로 찾음).
  - `a_worktree_follows_its_gitdir`: `<tmp>/wt/.git` 파일 = `gitdir: <tmp>/repo/.git/worktrees/wt\n`, 그 폴더의 `HEAD` = `ref: refs/heads/feat/x\n` → `Some("feat/x")`. 상대 경로 `gitdir: ../repo/.git/worktrees/wt`도 같은 결과.
  - `a_detached_head_shows_seven_characters`: `HEAD` = 40자 16진수 → 앞 7자.
  - `outside_a_repository_is_none`: 임시 폴더의 부모 쪽에 저장소가 있을 수 있으므로, 올라가는 단계 수를 받는 내부 함수(예: `fn branch_within(dir: &Path, levels: usize)`)로 `.git` 없는 폴더를 `levels` 0에서 시험 → `None`. `branch`는 64단계로 그것을 부른다. **(빌더: 내부 함수 이름)**
- [ ] **Step 2:** `cargo test --manifest-path native/Cargo.toml -- git::` → 컴파일 실패.
- [ ] **Step 3: 구현.** 위로 최대 64단계. `HEAD`와 `.git` 파일은 4 KiB까지만 읽음. 브랜치 이름은 1–255바이트, 제어 문자 없음일 때만. `git` 프로그램은 실행하지 않는다.
- [ ] **Step 4:** 같은 명령 → PASS. `cargo clippy …` 통과.
- [ ] **체크포인트(제안):** `git add native/src/git.rs native/src/main.rs native/build.rs && git commit -m "feat(host): read the git branch of a folder"`

### Task 2: 프로토콜 (`place` 메시지, 새 셸의 폴더와 시작)

**Files:**
- Modify: `native/src/protocol.rs`, `shared/protocol.ts`, `native/src/host/agent.rs` (`is_conversation_id` 사용처만)
- Test: `native/src/protocol.rs` tests, `tests/protocol.test.ts`

**Interfaces:**
- Produces (Rust):
  - `pub struct Place { pub cwd: String, pub branch: Option<String> }` (Serialize, Clone, PartialEq, Eq, Debug)
  - `ServerMessage::Place { place: Option<Place> }` → `{"type":"place","place":{"cwd":"/w","branch":"main"}}` / `{"type":"place","place":null}`
  - `pub struct Launch { pub kind: AgentKind, pub conversation: Option<String> }`
  - `AttachRequest` 새 필드 `pub cwd: Option<String>`, `pub launch: Option<Launch>`; `from_query`/`to_query`가 `cwd`, `agent`, `conversation`을 읽고 쓴다. `agent` 값은 `AgentKind`의 serde 이름(`claude`, `codex`; Task 3 뒤 `gemini`).
  - `pub fn is_conversation_id(text: &str) -> bool` (agent.rs에서 옮김, 규칙 그대로: 8–64자, 16진수와 `-`)
- Produces (TS):
  - `export interface Place { cwd: string; branch: string | null }`, `export function parsePlace(value: unknown): Place | null`
  - `ServerMessage`에 `{ type: "place"; place: Place | null }`
  - `export interface AgentLaunch { kind: AgentKind; conversation: string | null }`

- [ ] **Step 1: 실패하는 테스트**
  - Rust `place_messages_are_json`: 위 두 JSON 문자열과 정확히 같음.
  - Rust `attach_checks_cwd_and_launch`: 기본 쿼리(`session`, `kind=shell`, 크기)에
    - `cwd=/work`, `agent=claude` → `cwd == Some("/work")`, `launch == Some(Launch{Claude, None})`
    - `agent=codex&conversation=9f00f957-d4e0-4b3b-a88c-a76633c0e762` → conversation 있음
    - 거부(`None`): `cwd=work`(상대), `cwd`에 NUL, 4097바이트 `cwd`, `agent=bard`, `conversation=x;rm`, `conversation`만 있고 `agent` 없음, `kind=browser`에 `agent=claude`
    - `to_query` 후 `from_query`가 같은 값.
  - TS `place messages`: `parsePlace({cwd:"/w",branch:"main"})` → 그대로; `{cwd:"/w"}` → `branch: null`; `{cwd:""}`, `{cwd:3}`, `null` → `null`; 4097자 `branch` → `branch: null`.
- [ ] **Step 2:** `cargo test … -- protocol::` 와 `node --import tsx --test tests/protocol.test.ts` → 실패.
- [ ] **Step 3: 구현.** `cwd` 검사: `Path::is_absolute`, NUL 없음, ≤ 4096바이트.
- [ ] **Step 4:** 두 명령 PASS, `npm run type-check` 통과.
- [ ] **체크포인트(제안):** `git commit -m "feat(protocol): add the place message and the folder and agent of a new shell"`

### Task 3: Gemini CLI 감지

**Files:**
- Modify: `native/src/protocol.rs`(`AgentKind::Gemini`, serde `gemini`), `shared/protocol.ts`(`AgentKind`에 `"gemini"`, `parseAgentState`의 종류 목록), `native/src/host/agent.rs`, `src/i18n/messages.ts`(`agent.names.gemini` = `Gemini CLI`, 4개 언어 모두 같은 이름), `src/components/AgentCard.tsx`(대화 id를 모르는 에이전트: 지금 Codex 조건에 Gemini도), `src/styles.css`(`.agent-mark.agent-gemini`, `.agent-card-name.agent-gemini`: 색 변수 `--agent-gemini` 밝은/어두운 값 **(빌더: Gemini 파랑 `#3186FF` 계열에서 4.5:1 넘는 값)**)
- Test: `agent.rs` tests, `tests/agents.test.ts`

**Interfaces:**
- Consumes: Task 2 `is_conversation_id`
- Produces: `AgentKind::Gemini` / `"gemini"`; `AgentKind::label()` = `"Gemini CLI"`; `program()` = `"gemini"`. `resume_command`: `Some(id)` → `gemini --resume <id>`, `None` → `gemini --resume latest`, 뒤에 유지 옵션.
- 규칙 전부: 스펙 10절.

- [ ] **Step 1: 실패하는 테스트** (기존 테스트 모양을 따름: `argv("…")`, `record(…)`)
  - `finds_gemini_by_program_and_script`: `gemini` → Gemini; `/opt/homebrew/bin/gemini` → Gemini, `program` 경로 유지(기존 규칙대로); `node /opt/homebrew/lib/node_modules/@google/gemini-cli/bundle/gemini.js` → Gemini. 아님: `gemini -p hi`, `gemini --prompt hi`, `gemini mcp list`, `gemini extensions list`, `gemini skills list`, `gemini hooks list`, `gemini gemma`, `gemini --list-sessions`, `gemini --acp`, `gemini -v`.
  - `gemini_keeps_its_flags_and_drops_prompts_and_worktrees`: `gemini -m gemini-2.5-pro --yolo -w feat -i "fix it" --include-directories ../lib "start"` → 유지 `["-m","gemini-2.5-pro","--yolo","--include-directories","../lib"]`; `resume_command` = `gemini --resume latest -m gemini-2.5-pro --yolo --include-directories ../lib`.
  - `gemini_reads_its_conversation_from_the_command_line`: `gemini --resume 9f00f957-d4e0-4b3b-a88c-a76633c0e762` → session 있음, 명령 `gemini --resume 9f00f957-…`; `gemini --session-id <같은 id>` → 같음; `gemini -r latest`, `gemini -r 3` → session 없음.
  - `gemini_screens_tell_work_and_questions`: `screen_status("⠋ Thinking (esc to cancel, 3s)")` → Working; `"Allow execution of [Shell]?"` → Waiting; `"Apply this change?"` → Waiting.
  - TS `parseAgentState` 테스트에 `kind: "gemini"`가 통과함을 더함.
- [ ] **Step 2:** `cargo test … -- agent::` 와 `node --import tsx --test tests/agents.test.ts` → 실패.
- [ ] **Step 3: 구현.** `kept_flags`, `identify`, `tool_command`, `QUESTIONS`, `resume_command`에 Gemini를 더한다. 기존 Claude·Codex 테스트는 그대로 통과해야 한다.
- [ ] **Step 4:** PASS, `npm run type-check`, `cargo clippy`.
- [ ] **체크포인트(제안):** `git commit -m "feat(host): detect gemini cli and resume it after a restore"`

### Task 4: 호스트 (place 보내기, 새 셸을 폴더에서 시작하고 명령 입력)

**Files:**
- Modify: `native/src/host/agent.rs`, `native/src/host/session.rs`, `native/src/host/registry.rs`
- Test: `agent.rs` tests, `native/tests/integration.rs`

**Interfaces:**
- Consumes: Task 1 `git::branch`, Task 2 `Place`, `Launch`, `AttachRequest.cwd/launch`, `ServerMessage::Place`, Task 3 `AgentKind::Gemini`
- Produces: `pub fn launch_command(launch: &Launch) -> String` (agent.rs). 대화 없음 → `claude` / `codex` / `gemini`. 대화 있음 → `AgentRecord { kind, program: None, session: Some(id), args: vec![] }.resume_command()`와 같음.

- [ ] **Step 1: 실패하는 테스트**
  - 단위 `launch_commands`: `Claude,None` → `"claude"`; `Codex,None` → `"codex"`; `Gemini,None` → `"gemini"`; `Claude,Some(ID)` → `"claude --resume 9f00f957-…"`; `Codex,Some(ID)` → `"codex resume 9f00f957-…"`; `Gemini,Some(ID)` → `"gemini --resume 9f00f957-…"`.
  - 통합(unix) `a_launch_starts_the_agent_in_its_folder`: 기존 `FAKE_CLAUDE`와 `start_host_with`(가짜 `claude` 폴더를 `PATH` 앞에, `CLAUDE_CONFIG_DIR`)로 `agent=claude&cwd=<임시 폴더>` 셸을 연다. `agent_message(…, "working")`의 `agent.cwd`가 그 폴더(정규화한 경로)와 같음. 출력에 `FAKE_CLAUDE`가 한 번.
  - 통합 `a_launch_is_typed_once`: 같은 세션 id로 끊고 다시 붙는다(`reattached`). 2초 동안 출력에 `FAKE_CLAUDE`가 다시 나오지 않음.
  - 통합 `a_shell_reports_its_place`: 임시 폴더에 `.git/HEAD` = `ref: refs/heads/topic\n`, `cwd=<그 폴더>`로 셸을 열면 15초 안에 `{"type":"place","place":{"cwd":<정규화한 폴더>,"branch":"topic"}}`.
- [ ] **Step 2:** `cargo test … -- launch_commands a_launch_ a_shell_reports` → 실패.
- [ ] **Step 3: 구현**
  - registry: 새 세션(`meta`가 없을 때)만 `request.cwd`가 폴더면 그 폴더에서 시작(아니면 지금 규칙). `launch`는 `meta`가 없고 `kind == Shell`일 때만 넘긴다.
  - session: 시작 명령(`Option<String>`)을 받는 길을 더해, 기존 `Resume { input, at, latest }`로 입력한다(복원 안내 줄은 쓰지 않음). 대화가 있으면 `Tracker::resuming`으로 기록을 잡아 둔다. **(빌더: `start` 인자 추가 또는 `Spec` 필드)**
  - session: `poll_agent`에서 `Place { cwd: process_cwd(self.pid), branch: git::branch(&cwd) }`를 계산해 지난 값과 다르면 보낸다. 붙을 때(`attach`, 지금 `send_agent()` 옆)에도 보낸다.
- [ ] **Step 4:** 위 테스트와 기존 `a_restored_shell_resumes_its_agent` PASS. `cargo clippy` 통과.
- [ ] **체크포인트(제안):** `git commit -m "feat(host): start shells in a folder with an agent and report their place"`

### Task 5: 지난 대화 (`GET /api/agents/history`)

**Files:**
- Create: `native/src/server/agents.rs`
- Modify: `native/src/server/mod.rs`(라우트 두 곳: 대시보드와 `remote_router`), `native/src/server/docs.rs`(`read_allowed`를 `mod.rs`의 `pub(super)`로 옮겨 함께 씀), `native/src/server/remote_window.rs`(`/remote/{target}/api/agents/history` GET 전달, 쿼리 그대로)

**Interfaces:**
- Produces (Rust, 테스트용 `pub(crate)`):
  - `struct Conversation { kind: AgentKind, id: String, title: Option<String>, updated: u64 }` (Serialize: `{"kind","id","title","updated"}`, `updated`는 ms)
  - `fn claude_conversations(claude_dir: &Path, dir: &str, limit: usize) -> Vec<Conversation>`
  - `fn codex_conversations(codex_dir: &Path, dir: &str, limit: usize) -> Vec<Conversation>`
  - `fn gemini_conversations(gemini_dir: &Path, dir: &str, limit: usize) -> Vec<Conversation>` (`gemini_dir` = `.gemini` 폴더 자체)
  - `fn claude_folder_name(dir: &str) -> String` (영문자·숫자 아닌 글자 → `-`)
  - 폴더 찾기: Claude는 기존 `crate::host::agent::claude_home()` 규칙, Codex는 `CODEX_HOME` 아니면 `~/.codex`, Gemini는 `GEMINI_CLI_HOME`(있으면 `<그것>/.gemini`) 아니면 `~/.gemini`. 홈은 `crate::state_dir::home_dir()`.
  - 응답: `{"conversations":[…]}` 최근 순 5개. `dir`이 절대 경로가 아니면 400, `read_allowed` 아니면 403.
- Produces (TS, Task 7에서 씀): 경로 `/api/agents/history?dir=…`

- [ ] **Step 1: 실패하는 테스트** (임시 폴더에 가짜 기록; 한도 값은 스펙 6절)
  - `claude_folder_names`: `/Users/p/westerm/.claude/worktrees/x` → `-Users-p-westerm--claude-worktrees-x`.
  - `claude_titles_follow_their_order`: 한 파일에 `summary`, `ai-title`, `custom-title` 줄 → 제목은 `customTitle`. `custom-title` 없으면 마지막 `aiTitle`. 아무것도 없고 `last-prompt`만 → `lastPrompt`. 없으면 `None`. 제목의 줄바꿈은 공백, 121자 이상은 120자.
  - `claude_lists_the_newest_first`: 파일 세 개의 수정 시각을 다르게(`File::set_modified`) → 최근 순, `limit` 2면 2개. 이름이 id 모양이 아닌 `notes.jsonl`은 빠짐.
  - `claude_skips_conversations_of_another_folder`: 첫 `cwd`가 `/a/b-c`인 파일은 `/a/b/c` 요청에서 빠짐.
  - `codex_matches_the_folder_and_reads_titles`: `sessions/2026/11/06/rollout-…-<id>.jsonl` 첫 줄 `{"type":"session_meta","payload":{"id":"<id>","cwd":"/w"}}` 두 개(폴더 `/w`, `/x`), `session_index.jsonl`에 `{"id":"<id>","thread_name":"Fix fonts"}` → `/w` 요청에서 하나, 제목 `Fix fonts`.
  - `codex_scan_stops_at_its_limit`: 501개 파일 중 가장 오래된 하나만 폴더가 맞음 → 결과 없음.
  - `gemini_finds_the_project_by_name_or_root`: `projects.json` = `{"projects":{"/w":"w"}}`, `tmp/w/chats/session-2026-11-06T10-00-<8자>.jsonl` 첫 줄 `{"sessionId":"<id>","projectHash":"h","startTime":"…","kind":"main"}` → `/w`에서 하나. `projects.json`에 없고 `tmp/other/.project_root` = `/v`인 폴더 → `/v`에서 찾음.
  - `gemini_titles_and_subagents`: 둘째 줄 `{"type":"user","content":[{"text":"fix the fonts"}]}` → 제목 `fix the fonts`; 뒤에 `{"summary":"Font fix"}` 줄이 있으면 `Font fix`. 첫 줄 `kind` = `subagent`면 빠짐. `chats/<부모 id>/` 하위 폴더 파일은 안 봄. 옛 `.json` 파일 `{"sessionId":…,"summary":"Old","messages":[]}` → 제목 `Old`.
  - `broken_lines_and_files_are_skipped`: 깨진 JSON 줄, 빈 파일, 첫 줄 1 MiB 넘는 Codex 파일 → 패닉 없이 빠짐.
  - `history_needs_an_absolute_dir`: 라우트에 `dir=relative` → 400; 다른 출처(`Sec-Fetch-Site: cross-site`) → 403. (`server::tests::test_state` 사용)
- [ ] **Step 2:** `cargo test … -- agents::` → 실패.
- [ ] **Step 3: 구현.** 스펙 6절의 한도 값을 상수로: 파일당 16 MiB, Codex 파일 500개, 첫 줄 1 MiB, 색인 끝 4 MiB, Codex 첫 줄 메모리 기억 4096개, Gemini `.project_root` 256개, 결과 5개. 파일 읽기는 `spawn_blocking`.
- [ ] **Step 4:** PASS, `cargo clippy` 통과.
- [ ] **체크포인트(제안):** `git commit -m "feat(server): list the past agent conversations of a folder"`

### Task 6: 화면 상태 (`Session.place`, 새 셸의 폴더와 시작)

**Files:**
- Modify: `src/workspace.ts`, `src/terminal/connection.ts`, `src/terminal/TerminalView.tsx`, `src/components/Pane.tsx`
- Test: `tests/agents.test.ts`(기존 파일에 더함), `tests/protocol.test.ts`

**Interfaces:**
- Consumes: Task 2 `Place`, `parsePlace`, `AgentLaunch`
- Produces:
  - `Session`에 `place: Place | null`, `cwd: string | null`(새 셸의 폴더), `launch: AgentLaunch | null`. 셋 다 레이아웃에 저장하지 않는다.
  - `DashboardAction` `{ type: "place"; id: string; place: Place | null }`
  - `open` 액션에 `cwd?: string | null`, `launch?: AgentLaunch | null`
  - `ready`가 `launch`를 `null`로 지운다.
  - `connection.ts` 이벤트 `{ type: "place"; place: Place | null }`, 쿼리에 `cwd`, `agent`, `conversation`
- [ ] **Step 1: 실패하는 테스트**
  - `a shell keeps its place`: `place` 액션 뒤 세션의 `place`; `toSaved` 결과에 `place`, `cwd`, `launch`가 없음.
  - `ready clears the launch`: `open {kind:"shell", cwd:"/w", launch:{kind:"claude",conversation:null}}` → 세션에 그대로; `ready` 뒤 `launch === null`, `cwd`는 그대로.
  - `a new shell asks for its folder and agent`: `connection.ts`에서 쿼리를 만드는 부분을 내보낸 함수(예: `sessionQuery(options)`)로 꺼내, `cwd=/w&agent=gemini&conversation=<id>`를 담고 `launch`가 없으면 `agent`가 없음을 확인. **(빌더: 함수 이름)**
- [ ] **Step 2:** `node --import tsx --test tests/agents.test.ts tests/protocol.test.ts` → 실패.
- [ ] **Step 3: 구현.** `Pane` → `TerminalView` → `connect`로 `cwd`, `launch`를 넘긴다(`cwdFrom`과 같은 길). `place` 이벤트는 `dispatch({type:"place"})`.
- [ ] **Step 4:** PASS, `npm run type-check` 통과.
- [ ] **체크포인트(제안):** `git commit -m "feat(ui): keep each shell's place and start new shells in a folder"`

### Task 7: 패널의 순수 로직 (`src/agents.ts`)

**Files:**
- Create: `src/agents.ts`
- Test: `tests/agent-panel.test.ts` (새)

**Interfaces:**
- Consumes: Task 6 `Session.place`, 기존 `agentDisplay`, `sessionLabel`, `homeShort`(`src/components/AgentCard.tsx`), `REMOTE_TARGET`(`src/remoteWindow.ts`)
- Produces:
  - `export interface AgentFolder { key: string; path: string | null; name: string; parent: string; branch: string | null; shells: number; agents: Session[]; waiting: boolean }` (`key`: 경로, 모르면 `""`)
  - `export function agentFolders(state: Dashboard): AgentFolder[]` — 화면 워크스페이스의 셸(`kind === "shell"`)만. 세션 폴더 = `agent?.cwd ?? place?.cwd`. 탭 순서·패널 순서로 처음 나온 폴더 순. 폴더 안 에이전트는 `agentDisplay` 순서 `waiting, done, working, idle`, 같으면 패널 순. `branch`는 그 폴더 셸의 `place.branch` 중 처음 것.
  - `export function folderLabel(path: string): { name: string; parent: string }` — `homeShort` 뒤: `/Users/p` → `{name:"~", parent:""}`, `/Users/p/westerm` → `{name:"westerm", parent:"~/"}`, `/Users/p/westerm/.claude/worktrees/x` → `{name:"x", parent:"worktrees/"}`, `/srv/app` → `{name:"app", parent:"srv/"}`, `/` → `{name:"/", parent:""}`.
  - `export type SidebarView = "sessions" | "agents" | null`
  - `export interface SidebarFlags { layout: "top" | "sidebar"; narrow: boolean; sidebarOpen: boolean; drawerOpen: boolean; agentsView: boolean }`
  - `export function sidebarView(o: SidebarFlags): SidebarView` — 열림: 좁으면 `drawerOpen`, 위쪽 레이아웃이면 `agentsView`, 아니면 `sidebarOpen`. 열렸을 때 위쪽 레이아웃은 늘 `"agents"`, 사이드바 레이아웃은 `agentsView ? "agents" : "sessions"`.
  - `export function pressAgents(o: SidebarFlags): Pick<SidebarFlags, "agentsView" | "sidebarOpen" | "drawerOpen">` — 지금 보기가 `"agents"`면: 좁으면 서랍 닫음, 위쪽이면 `agentsView: false`, 사이드바 레이아웃이면 `agentsView: false`(세션 목록으로, 열린 채). 아니면 `agentsView: true`와 함께 연다(좁으면 `drawerOpen`, 사이드바 레이아웃이면 `sidebarOpen`).
  - `export function elsewhereWaiting(state: Dashboard): number` — 화면 밖 워크스페이스의 `waiting` 에이전트 수.
  - `export function anyWaiting(state: Dashboard): boolean`
  - `export interface PastConversation { kind: AgentKind; id: string; title: string | null; updated: number }`
  - `export function pastConversations(list: PastConversation[], state: Dashboard): PastConversation[]` — 모든 세션의 `agent.conversation`을 뺀다.
  - `export async function fetchHistory(dir: string): Promise<PastConversation[] | null>` — 원격 창이면 `/remote/<대상>/api/agents/history` (`src/docs/api.ts`의 `docsPath`와 같은 방식), 실패하면 `null`. 응답의 잘못된 항목은 버린다.
- [ ] **Step 1: 실패하는 테스트**
  - `folders group shells by their place`: 탭 2개, 셸 4개(두 개는 같은 폴더, 하나는 에이전트 `cwd`가 셸 `place`와 다름 → 에이전트 폴더로, 하나는 place 없음 → `key ""`), SSH 하나와 문서 패널 하나는 빠짐. 폴더 순서·`shells`·`agents` 수 확인.
  - `folders keep pane order when states change`: 두 폴더의 에이전트 상태를 바꿔도 폴더 순서 같음; 폴더 안에서는 `waiting`이 앞.
  - `only the workspace in view`: 다른 워크스페이스의 셸은 빠짐; `elsewhereWaiting`은 그쪽 `waiting` 수; `anyWaiting`은 모든 워크스페이스.
  - `folder labels`: 위 다섯 경우.
  - `the agents button`: (top, wide) 닫힘→열림→닫힘; (sidebar, wide, 세션 목록 열림) → agents → sessions; (sidebar, wide, 닫힘) → agents 열림; (narrow, 두 레이아웃) 서랍 닫힘 → 서랍 열림·agents → 서랍 닫힘.
  - `running conversations are not past`: 세션 하나의 `agent.conversation`이 목록의 id와 같으면 빠짐.
- [ ] **Step 2:** `node --import tsx --test tests/agent-panel.test.ts` → 실패.
- [ ] **Step 3: 구현.**
- [ ] **Step 4:** PASS, `npm run type-check`.
- [ ] **체크포인트(제안):** `git commit -m "feat(ui): group a workspace's shells and agents by folder"`

### Task 8: 실제 AI 로고 (`src/agentIcons.tsx`, `AgentMark`)

**Files:**
- Create: `src/agentIcons.tsx`
- Modify: `src/components/AgentMark.tsx`(`Sparkle` → `AgentIcon`), `src/styles.css`(`.agent-mark` 안 아이콘 크기; Claude·Gemini는 SVG 자체 색, Codex는 `currentColor` = `var(--text)`)
- Test: `tests/agent-panel.test.ts`

**Interfaces:**
- Produces: `export function AgentIcon({ kind, size }: { kind: AgentKind; size: number }): JSX.Element` — `aria-hidden="true"`, `viewBox="0 0 24 24"`, `width`/`height` = `size`.
  - Claude: `claude-color.svg`의 path, `fill="#D97757"`.
  - Codex: `openai.svg`의 path, `fill="currentColor"`, `fillRule="evenodd"`.
  - Gemini: `gemini-color.svg`의 path 네 개와 그라디언트 세 개. 그라디언트 id는 `useId()`로 아이콘마다 다르게.
  - 원문: `https://cdn.jsdelivr.net/npm/@lobehub/icons-static-svg@1.95.1/icons/{claude-color,openai,gemini-color}.svg`. path 데이터는 손으로 고치지 않는다. 파일 머리 주석: `Logos from @lobehub/icons-static-svg@1.95.1 (MIT). Claude is a trademark of Anthropic, OpenAI of OpenAI, Gemini of Google.`
- [ ] **Step 1: 실패하는 테스트** (`.ts` 파일이라 JSX 대신 `createElement`, `react-dom/server`의 `renderToStaticMarkup`): Claude 마크업은 `#D97757`과 `width="16"`, Codex는 `currentColor`, Gemini는 `#3186FF`를 담음. Gemini 둘을 한 마크업에 그리면 그라디언트 id가 서로 다름. `AgentMark`의 마크업에 `lucide-sparkle`이 없음. `useI18n`을 쓰는 컴포넌트는 기존 i18n 제공자로 감싼다(기본값이 있으면 그대로) **(빌더)**.
- [ ] **Step 2:** 실행 → 실패.
- [ ] **Step 3: 구현.** 밝은·어두운 테마에서 로고가 보이는지는 Task 10 스크린샷에서 확인.
- [ ] **Step 4:** PASS, `npm run type-check`.
- [ ] **체크포인트(제안):** `git commit -m "feat(ui): show the Claude, OpenAI, and Gemini logos for agents"`

### Task 9: 에이전트 패널과 활동 바 버튼

**Files:**
- Create: `src/components/AgentPanel.tsx`, `src/useAgentsView.ts`(`localStorage` 키 `westerm.agentsView`, `useSidebarWidth`와 같은 try/catch 규칙)
- Modify: `src/components/ActivityBar.tsx`, `src/App.tsx`, `src/styles.css`, `src/i18n/messages.ts`(새 `agents` 묶음, 스펙 8절 문구 4개 언어)
- Test: `tests/agent-panel.test.ts`, 기존 `tests/i18n.test.ts`

**Interfaces:**
- Consumes: Task 7 전부, Task 8 `AgentIcon`, 기존 `WorkspaceMenu`(`where="sidebar"`), `AgentMark`, `minutesSince`
- Produces:
  - `AgentPanel` props: `{ state: Dashboard; renaming: RenameTarget | null; onRename; onSelectWorkspace(id); onNewWorkspace(); onCloseWorkspace(id); onRenameWorkspace(id, name); onSetWorkspaceIcon(id, icon); onSetWorkspaceColor(id, color); onFocus(id: string); onStart(folder: string, launch: AgentLaunch); compact: boolean; history?: (dir: string) => Promise<PastConversation[] | null> }` — 워크스페이스 메뉴 props는 `SessionList`와 같은 모양. `history` 기본값은 `fetchHistory`(테스트에서 바꿔 끼움).
  - `ActivityBar` 새 props: `agentsOpen: boolean; agentsWaiting: boolean; onToggleAgents: () => void`. 버튼은 `aria-pressed`, 노란 점은 `agentsWaiting`일 때.
  - `App`: `onStart`는 `dispatch({ type: "open", id: newId("s"), kind: "shell", cwd: folder, launch, aspect: layerAspect() })` 후 서랍을 닫는다. 사이드바 자리는 Task 7 `sidebarView`로 `SessionList` 또는 `AgentPanel`. 활동 바 버튼은 `pressAgents`.
- [ ] **Step 1: 실패하는 테스트**
  - en에만 새 키를 넣고 `tests/i18n.test.ts` 실행 → 실패(다른 언어에 키 없음).
  - `tests/agent-panel.test.ts`에 `renderToStaticMarkup`로: 폴더 두 개(에이전트 있음/없음)인 상태의 `AgentPanel` 마크업이 (1) 폴더 이름과 `~/` 앞 경로, (2) 브랜치, (3) "No agents"와 시작 버튼 세 개(`aria-label` "Start Claude Code here", "Start Codex here", "Start Gemini CLI here"), (4) 입력 필요 행의 "Needs your input"을 담음. `history`는 빈 배열을 돌려주는 함수(정적 렌더에서 네트워크 없음).
- [ ] **Step 2:** 실행 → 실패.
- [ ] **Step 3: 구현**
  - 패널 구조: 40px 워크스페이스 메뉴 줄(+ "Elsewhere N") → 32px 제목 줄("Folders N", "N agents") → 폴더 트리. 행 28px, 기존 `.tree-row`, `.tree-children` 스타일을 다시 쓴다. 값은 목업 A4.
  - 에이전트 행: 왼쪽 `AgentIcon`(16px), 이름, 오른쪽 상태 표시(입력 필요 깜빡이는 노란 점, 작업 중 도는 파란 원, 완료 초록 체크, 대기 회색 점; `prefers-reduced-motion`에서 정지)와 `since` 시간. 누르면 `onFocus`. 포커스된 패널의 행은 워크스페이스 색(`--tab-color-soft`).
  - 시작 버튼: 에이전트가 없는 폴더는 늘 보이고, 에이전트가 있는 폴더는 폴더 행에 마우스를 올리거나 키보드 포커스가 있을 때 보인다.
  - 지난 대화: 폴더마다 `history(path)`. 패널이 열릴 때, 그리고 그 폴더의 돌던 대화 id 목록이 바뀔 때 다시 읽음. 처음엔 펼침. 각 줄: `AgentIcon`(14px), 제목(없으면 "Untitled conversation"), 언제, "Resume" 버튼(→ `onStart(path, { kind, conversation: id })`).
  - 좁게 끌었을 때(`compact`): 폴더는 폴더 아이콘, 에이전트는 `AgentMark`. 이름은 툴팁.
  - 모든 셸의 `place`가 없을 때만 맨 아래 "Agents show on macOS and Linux".
- [ ] **Step 4:** `npm test`, `npm run type-check` PASS.
- [ ] **체크포인트(제안):** `git commit -m "feat(ui): add the agents sidebar"`

### Task 10: 문서, 변경 기록, 수동 확인

**Files:**
- Modify: `docs/usage.md`(새 "Agents sidebar" 절: 여는 법, 폴더 묶기 규칙, 상태 표, 시작·이어서, 지난 대화 출처와 한도, Windows; 기존 "AI agents" 절에 Gemini CLI), `docs/sessions.md`(Gemini 감지와 재개 표, 셸 place 보고, 새 셸의 시작 명령은 새 세션에만), `docs/architecture.md`(`git.rs`, `server/agents.rs`, 보안 절에 지난 대화 경로), `CHANGELOG.md` `[Unreleased]` 두 줄(사이드바, Gemini)

- [ ] **Step 1:** 문서 작성. 문구는 화면 문구와 같게.
- [ ] **Step 2:** 전체 확인 네 명령(`npm test`, `npm run type-check`, `npm run build`, `cargo clippy …`) 통과.
- [ ] **Step 3: 수동 확인** — `WESTERM_STATE_DIR=/tmp/wa ./native/target/release/westerm --port 4318`. 임시 git 저장소 두 개에서 셸을 열고, 한쪽에서 `claude`를 실행. 패널에서 (1) 폴더 묶음과 브랜치, (2) 상태 변화, (3) "여기서 시작"으로 Codex·Gemini 시작, (4) 지난 대화 "이어서", (5) 위쪽·사이드바 레이아웃 전환, (6) 640px 이하 서랍을 본다. 밝은·어두운 테마 스크린샷을 남긴다. 끝나면 같은 상태 폴더로 `westerm stop`, 폴더 삭제.
- [ ] **체크포인트(제안):** `git commit -m "docs(agents): describe the agents sidebar and gemini cli"`
