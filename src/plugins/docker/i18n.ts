// The Docker plugin's own text in every UI language. English follows the
// spec word for word; the others follow the tone of src/i18n/messages.ts.

import type { Locale } from "../../i18n/messages.ts";
import type { ContainerStateName } from "./model.ts";

const en = {
  name: "Docker",
  description:
    "See your containers by compose project. Open logs and shells as westerm panes. Click a port to open it in a browser pane.",
  permissions: [
    "Run docker commands on this computer",
    "Open terminal and browser panes",
    "Check containers in the background",
  ],
  /** The engine name in the header: this computer's Docker. */
  local: "local",
  refresh: "Refresh",
  more: "More actions",
  uninstall: "Uninstall…",
  filter: "Filter containers",
  counts: (running: number, total: number) => `${running} / ${total} running`,
  tree: "Containers",
  other: "Other containers",
  noContainers: "No containers",
  noContainersHint: "Containers you start with docker or docker compose show up here.",
  noMatch: "No containers match the filter",
  notRunning: "Docker is not running",
  noCli: "Docker CLI not found",
  noCliHint: "Install Docker, and your containers show up here.",
  installGuide: "Docker install guide",
  retry: "Retry",
  copy: "Copy",
  copied: "Copied",
  copyFailed: "Could not copy",
  logs: "Logs",
  shell: "Shell",
  openPort: (port: number) => `Open :${port} in browser`,
  start: "Start",
  stop: "Stop",
  restart: "Restart",
  removeMenu: "Remove…",
  remove: "Remove",
  cancel: "Cancel",
  copyId: "Copy container ID",
  startAll: "Start all",
  stopAll: "Stop all",
  confirmRemove: (name: string) => `Remove ${name}? This deletes the container. Its image and volumes stay.`,
  /** An action button's name: what it does to which container or project. */
  on: (action: string, name: string) => `${action} ${name}`,
  menu: (name: string) => `Actions for ${name}`,
  states: {
    running: "Running",
    exited: "Exited",
    paused: "Paused",
    restarting: "Restarting",
    created: "Created",
    dead: "Dead",
  } as Record<ContainerStateName, string>,
  healthy: "healthy",
  unhealthy: "unhealthy",
  loading: "Loading…",
  busy: "Working…",
  exited: (code: number, ago: string | null) => `Exited (${code})${ago ? ` · ${ago}` : ""}`,
  ago: (minutes: number) =>
    minutes < 1
      ? "just now"
      : minutes < 60
        ? `${minutes} min ago`
        : minutes < 24 * 60
          ? `${Math.floor(minutes / 60)} h ago`
          : `${Math.floor(minutes / (24 * 60))} d ago`,
  requestFailed: "Request failed",
};

export type DockerMessages = typeof en;

const ko: DockerMessages = {
  name: "Docker",
  description:
    "compose 프로젝트별로 컨테이너를 봅니다. 로그와 셸을 westerm 패널로 엽니다. 포트를 클릭하면 브라우저 패널에서 엽니다.",
  permissions: ["이 컴퓨터에서 docker 명령 실행", "터미널과 브라우저 패널 열기", "백그라운드에서 컨테이너 확인"],
  local: "local",
  refresh: "새로 고침",
  more: "더 보기",
  uninstall: "제거…",
  filter: "컨테이너 찾기",
  counts: (running, total) => `실행 중 ${running} / 전체 ${total}`,
  tree: "컨테이너",
  other: "기타 컨테이너",
  noContainers: "컨테이너가 없습니다",
  noContainersHint: "docker나 docker compose로 시작한 컨테이너가 여기에 보입니다.",
  noMatch: "맞는 컨테이너가 없습니다",
  notRunning: "Docker가 실행 중이 아닙니다",
  noCli: "Docker CLI를 찾을 수 없습니다",
  noCliHint: "Docker를 설치하면 컨테이너가 여기에 보입니다.",
  installGuide: "Docker 설치 안내",
  retry: "다시 시도",
  copy: "복사",
  copied: "복사됨",
  copyFailed: "복사하지 못했습니다",
  logs: "로그",
  shell: "셸",
  openPort: (port) => `브라우저에서 :${port} 열기`,
  start: "시작",
  stop: "정지",
  restart: "재시작",
  removeMenu: "제거…",
  remove: "제거",
  cancel: "취소",
  copyId: "컨테이너 ID 복사",
  startAll: "모두 시작",
  stopAll: "모두 정지",
  confirmRemove: (name) => `${name}을(를) 제거할까요? 컨테이너를 지웁니다. 이미지와 볼륨은 남습니다.`,
  on: (action, name) => `${name} ${action}`,
  menu: (name) => `${name} 동작`,
  states: {
    running: "실행 중",
    exited: "종료됨",
    paused: "일시 정지",
    restarting: "재시작 중",
    created: "생성됨",
    dead: "죽음",
  },
  healthy: "healthy",
  unhealthy: "unhealthy",
  loading: "불러오는 중…",
  busy: "처리 중…",
  exited: (code, ago) => `종료됨 (${code})${ago ? ` · ${ago}` : ""}`,
  ago: (minutes) =>
    minutes < 1
      ? "방금"
      : minutes < 60
        ? `${minutes}분 전`
        : minutes < 24 * 60
          ? `${Math.floor(minutes / 60)}시간 전`
          : `${Math.floor(minutes / (24 * 60))}일 전`,
  requestFailed: "요청이 실패했습니다",
};

const ja: DockerMessages = {
  name: "Docker",
  description:
    "コンテナを compose プロジェクトごとに表示します。ログとシェルを westerm のペインで開きます。ポートをクリックするとブラウザーペインで開きます。",
  permissions: ["このコンピューターで docker コマンドを実行", "ターミナルとブラウザーのペインを開く", "バックグラウンドでコンテナを確認"],
  local: "local",
  refresh: "更新",
  more: "その他の操作",
  uninstall: "アンインストール…",
  filter: "コンテナを絞り込む",
  counts: (running, total) => `実行中 ${running} / 全 ${total}`,
  tree: "コンテナ",
  other: "その他のコンテナ",
  noContainers: "コンテナがありません",
  noContainersHint: "docker や docker compose で起動したコンテナがここに表示されます。",
  noMatch: "一致するコンテナがありません",
  notRunning: "Docker が実行されていません",
  noCli: "Docker CLI が見つかりません",
  noCliHint: "Docker をインストールすると、コンテナがここに表示されます。",
  installGuide: "Docker のインストールガイド",
  retry: "再試行",
  copy: "コピー",
  copied: "コピーしました",
  copyFailed: "コピーできませんでした",
  logs: "ログ",
  shell: "シェル",
  openPort: (port) => `:${port} をブラウザーで開く`,
  start: "開始",
  stop: "停止",
  restart: "再起動",
  removeMenu: "削除…",
  remove: "削除",
  cancel: "キャンセル",
  copyId: "コンテナ ID をコピー",
  startAll: "すべて開始",
  stopAll: "すべて停止",
  confirmRemove: (name) => `${name} を削除しますか？コンテナを削除します。イメージとボリュームは残ります。`,
  on: (action, name) => `${action}: ${name}`,
  menu: (name) => `${name} の操作`,
  states: {
    running: "実行中",
    exited: "終了",
    paused: "一時停止",
    restarting: "再起動中",
    created: "作成済み",
    dead: "停止不能",
  },
  healthy: "healthy",
  unhealthy: "unhealthy",
  loading: "読み込み中…",
  busy: "処理中…",
  exited: (code, ago) => `終了 (${code})${ago ? ` · ${ago}` : ""}`,
  ago: (minutes) =>
    minutes < 1
      ? "たった今"
      : minutes < 60
        ? `${minutes} 分前`
        : minutes < 24 * 60
          ? `${Math.floor(minutes / 60)} 時間前`
          : `${Math.floor(minutes / (24 * 60))} 日前`,
  requestFailed: "リクエストに失敗しました",
};

const zh: DockerMessages = {
  name: "Docker",
  description: "按 compose 项目查看容器。在 westerm 面板中打开日志和 Shell。点击端口即可在浏览器面板中打开。",
  permissions: ["在此计算机上运行 docker 命令", "打开终端和浏览器面板", "在后台检查容器"],
  local: "local",
  refresh: "刷新",
  more: "更多操作",
  uninstall: "卸载…",
  filter: "筛选容器",
  counts: (running, total) => `运行中 ${running} / 共 ${total}`,
  tree: "容器",
  other: "其他容器",
  noContainers: "没有容器",
  noContainersHint: "用 docker 或 docker compose 启动的容器会显示在这里。",
  noMatch: "没有匹配的容器",
  notRunning: "Docker 未运行",
  noCli: "未找到 Docker CLI",
  noCliHint: "安装 Docker 后，容器会显示在这里。",
  installGuide: "Docker 安装指南",
  retry: "重试",
  copy: "复制",
  copied: "已复制",
  copyFailed: "无法复制",
  logs: "日志",
  shell: "Shell",
  openPort: (port) => `在浏览器中打开 :${port}`,
  start: "启动",
  stop: "停止",
  restart: "重启",
  removeMenu: "删除…",
  remove: "删除",
  cancel: "取消",
  copyId: "复制容器 ID",
  startAll: "全部启动",
  stopAll: "全部停止",
  confirmRemove: (name) => `删除 ${name}？这会删除该容器，其镜像和卷会保留。`,
  on: (action, name) => `${action} ${name}`,
  menu: (name) => `${name} 的操作`,
  states: {
    running: "运行中",
    exited: "已退出",
    paused: "已暂停",
    restarting: "正在重启",
    created: "已创建",
    dead: "已失效",
  },
  healthy: "healthy",
  unhealthy: "unhealthy",
  loading: "加载中…",
  busy: "处理中…",
  exited: (code, ago) => `已退出 (${code})${ago ? ` · ${ago}` : ""}`,
  ago: (minutes) =>
    minutes < 1
      ? "刚刚"
      : minutes < 60
        ? `${minutes} 分钟前`
        : minutes < 24 * 60
          ? `${Math.floor(minutes / 60)} 小时前`
          : `${Math.floor(minutes / (24 * 60))} 天前`,
  requestFailed: "请求失败",
};

export const DOCKER_TEXT: Record<Locale, DockerMessages> = { en, ko, ja, zh };
