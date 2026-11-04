// Menu and message text in the system language: Korean, Japanese, Chinese,
// or English. Keep in step with the dashboard's languages (src/i18n).

import Foundation

enum L {
    static let language: String = {
        let code = String((Locale.preferredLanguages.first ?? "en").prefix(2))
        return ["ko", "ja", "zh"].contains(code) ? code : "en"
    }()

    private static func pick(_ en: String, _ ko: String, _ ja: String, _ zh: String) -> String {
        switch language {
        case "ko": ko
        case "ja": ja
        case "zh": zh
        default: en
        }
    }

    // Menus
    static var about: String { pick("About westerm", "westerm 정보", "westerm について", "关于 westerm") }
    static var checkForUpdates: String { pick("Check for Updates…", "업데이트 확인…", "アップデートを確認…", "检查更新…") }
    static var settings: String { pick("Settings…", "설정…", "設定…", "设置…") }
    static var hide: String { pick("Hide westerm", "westerm 가리기", "westerm を隠す", "隐藏 westerm") }
    static var hideOthers: String { pick("Hide Others", "기타 가리기", "ほかを隠す", "隐藏其他") }
    static var showAll: String { pick("Show All", "모두 보기", "すべてを表示", "全部显示") }
    static var quit: String { pick("Quit westerm", "westerm 종료", "westerm を終了", "退出 westerm") }
    static var edit: String { pick("Edit", "편집", "編集", "编辑") }
    static var undo: String { pick("Undo", "실행 취소", "取り消す", "撤销") }
    static var redo: String { pick("Redo", "실행 복귀", "やり直す", "重做") }
    static var cut: String { pick("Cut", "오려두기", "カット", "剪切") }
    static var copy: String { pick("Copy", "복사하기", "コピー", "拷贝") }
    static var paste: String { pick("Paste", "붙여넣기", "ペースト", "粘贴") }
    static var selectAll: String { pick("Select All", "전체 선택", "すべてを選択", "全选") }
    static var view: String { pick("View", "보기", "表示", "显示") }
    static var reload: String { pick("Reload", "새로고침", "再読み込み", "重新载入") }
    static var openInBrowser: String { pick("Open in Browser", "브라우저에서 열기", "ブラウザで開く", "在浏览器中打开") }
    static var fullScreen: String { pick("Enter Full Screen", "전체 화면 시작", "フルスクリーンにする", "进入全屏幕") }
    static var window: String { pick("Window", "윈도우", "ウインドウ", "窗口") }
    static var minimize: String { pick("Minimize", "최소화", "しまう", "最小化") }
    static var zoom: String { pick("Zoom", "확대/축소", "拡大/縮小", "缩放") }
    static var close: String { pick("Close", "닫기", "閉じる", "关闭") }
    static var nextTab: String { pick("Show Next Tab", "다음 탭 보기", "次のタブを表示", "显示下一个标签页") }
    static var previousTab: String { pick("Show Previous Tab", "이전 탭 보기", "前のタブを表示", "显示上一个标签页") }
    static var nextPane: String { pick("Next Pane", "다음 패널", "次のパネル", "下一个面板") }
    static var previousPane: String { pick("Previous Pane", "이전 패널", "前のパネル", "上一个面板") }
    static var goToTab: String { pick("Go to Tab", "탭으로 이동", "タブへ移動", "转到标签页") }
    static func tabNumber(_ number: Int) -> String { pick("Tab \(number)", "탭 \(number)", "タブ \(number)", "标签页 \(number)") }
    static var lastTab: String { pick("Last Tab", "마지막 탭", "最後のタブ", "最后一个标签页") }

    // Window messages
    static var starting: String { pick("Starting westerm…", "westerm을 시작하는 중…", "westerm を起動中…", "正在启动 westerm…") }
    static func cannotStart(_ log: String) -> String {
        pick("westerm could not start. See \(log).", "westerm을 시작하지 못했습니다. \(log)를 확인하세요.",
             "westerm を起動できませんでした。\(log) を確認してください。", "westerm 无法启动。请查看 \(log)。")
    }
    static func stopped(_ log: String) -> String {
        pick("westerm stopped. See \(log).", "westerm이 멈췄습니다. \(log)를 확인하세요.",
             "westerm が停止しました。\(log) を確認してください。", "westerm 已停止。请查看 \(log)。")
    }

    // Updates
    static var checkFailed: String { pick("Could not check for updates", "업데이트를 확인하지 못했습니다", "アップデートを確認できませんでした", "无法检查更新") }
    static func upToDate(_ version: String) -> String {
        pick("westerm \(version) is up to date", "최신 버전입니다 (\(version))", "最新バージョンです (\(version))", "已是最新版本 (\(version))")
    }
    static func available(_ version: String) -> String {
        pick("westerm \(version) is available", "westerm \(version) 버전이 나왔습니다", "westerm \(version) が公開されました", "westerm \(version) 已发布")
    }
    static func availableDetail(_ current: String) -> String {
        pick("You have \(current). The app restarts after the update, and open shells come back once as \"restored\".",
             "지금 버전은 \(current)입니다. 업데이트하면 앱이 다시 시작하고, 열린 셸은 한 번 \"복원됨\"으로 다시 시작합니다.",
             "現在のバージョンは \(current) です。更新後にアプリが再起動し、開いているシェルは一度「復元済み」として再起動します。",
             "当前版本为 \(current)。更新后应用会重新启动，已打开的 Shell 会以“已恢复”状态重新启动一次。")
    }
    static var install: String { pick("Update", "업데이트", "更新", "更新") }
    static var later: String { pick("Later", "나중에", "後で", "稍后") }
    static var releaseNotes: String { pick("Release Notes", "변경 사항", "変更内容", "更新内容") }
    static var updating: String { pick("Updating…", "업데이트하는 중…", "更新中…", "正在更新…") }
    static var updateFailed: String { pick("The update failed", "업데이트하지 못했습니다", "更新できませんでした", "更新失败") }
    static var moveToApplications: String {
        pick("Move westerm.app to the Applications folder, then update.", "westerm.app을 응용 프로그램 폴더로 옮긴 뒤 업데이트하세요.",
             "westerm.app をアプリケーションフォルダに移動してから更新してください。", "请先把 westerm.app 移到“应用程序”文件夹，再更新。")
    }
    static func cannotWrite(_ folder: String) -> String {
        pick("Cannot write \(folder).", "\(folder)에 쓸 수 없습니다.", "\(folder) に書き込めません。", "无法写入 \(folder)。")
    }
    static var checksumMismatch: String {
        pick("The download does not match its SHA-256 sum.", "내려받은 파일이 SHA-256 값과 다릅니다.",
             "ダウンロードしたファイルが SHA-256 と一致しません。", "下载的文件与 SHA-256 不一致。")
    }
    static var cannotUnpack: String { pick("Could not unpack the download.", "내려받은 파일을 풀지 못했습니다.", "ダウンロードを展開できませんでした。", "无法解压下载的文件。") }
    static var wrongVersion: String { pick("The download is not the expected version.", "내려받은 앱의 버전이 맞지 않습니다.", "ダウンロードしたアプリのバージョンが違います。", "下载的应用版本不符。") }
    static var noAppDownload: String { pick("The release has no macOS app.", "이 릴리스에 macOS 앱이 없습니다.", "このリリースに macOS アプリがありません。", "此版本没有 macOS 应用。") }
}
