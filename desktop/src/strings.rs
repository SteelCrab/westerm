//! The macOS menu bar's text in the system language: Korean, Japanese,
//! Chinese, or English. Keep in step with the dashboard's languages
//! (src/i18n). The start page (ui/index.html) has its own copy of its few lines.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    En,
    Ko,
    Ja,
    Zh,
}

impl Language {
    /// The language of a system language tag such as "ko-KR" or "zh-Hans".
    pub fn from_tag(tag: &str) -> Self {
        match tag.get(..2).map(str::to_ascii_lowercase).as_deref() {
            Some("ko") => Self::Ko,
            Some("ja") => Self::Ja,
            Some("zh") => Self::Zh,
            _ => Self::En,
        }
    }

    pub fn system() -> Self {
        sys_locale::get_locale().map_or(Self::En, |tag| Self::from_tag(&tag))
    }

    fn pick(self, en: &'static str, ko: &'static str, ja: &'static str, zh: &'static str) -> &'static str {
        match self {
            Self::En => en,
            Self::Ko => ko,
            Self::Ja => ja,
            Self::Zh => zh,
        }
    }
}

pub struct MenuText {
    pub about: &'static str,
    pub check_for_updates: &'static str,
    pub settings: &'static str,
    pub hide: &'static str,
    pub hide_others: &'static str,
    pub show_all: &'static str,
    pub quit: &'static str,
    pub edit: &'static str,
    pub undo: &'static str,
    pub redo: &'static str,
    pub cut: &'static str,
    pub copy: &'static str,
    pub paste: &'static str,
    pub select_all: &'static str,
    pub view: &'static str,
    pub reload: &'static str,
    pub open_in_browser: &'static str,
    pub full_screen: &'static str,
    pub window: &'static str,
    pub minimize: &'static str,
    pub zoom: &'static str,
    pub close: &'static str,
    pub next_tab: &'static str,
    pub previous_tab: &'static str,
    pub next_pane: &'static str,
    pub previous_pane: &'static str,
    pub go_to_tab: &'static str,
    pub last_tab: &'static str,
    language: Language,
}

impl MenuText {
    pub fn new(l: Language) -> Self {
        Self {
            about: l.pick("About westerm", "westerm 정보", "westerm について", "关于 westerm"),
            check_for_updates: l.pick("Check for Updates…", "업데이트 확인…", "アップデートを確認…", "检查更新…"),
            settings: l.pick("Settings…", "설정…", "設定…", "设置…"),
            hide: l.pick("Hide westerm", "westerm 가리기", "westerm を隠す", "隐藏 westerm"),
            hide_others: l.pick("Hide Others", "기타 가리기", "ほかを隠す", "隐藏其他"),
            show_all: l.pick("Show All", "모두 보기", "すべてを表示", "全部显示"),
            quit: l.pick("Quit westerm", "westerm 종료", "westerm を終了", "退出 westerm"),
            edit: l.pick("Edit", "편집", "編集", "编辑"),
            undo: l.pick("Undo", "실행 취소", "取り消す", "撤销"),
            redo: l.pick("Redo", "실행 복귀", "やり直す", "重做"),
            cut: l.pick("Cut", "오려두기", "カット", "剪切"),
            copy: l.pick("Copy", "복사하기", "コピー", "拷贝"),
            paste: l.pick("Paste", "붙여넣기", "ペースト", "粘贴"),
            select_all: l.pick("Select All", "전체 선택", "すべてを選択", "全选"),
            view: l.pick("View", "보기", "表示", "显示"),
            reload: l.pick("Reload", "새로고침", "再読み込み", "重新载入"),
            open_in_browser: l.pick("Open in Browser", "브라우저에서 열기", "ブラウザで開く", "在浏览器中打开"),
            full_screen: l.pick("Enter Full Screen", "전체 화면 시작", "フルスクリーンにする", "进入全屏幕"),
            window: l.pick("Window", "윈도우", "ウインドウ", "窗口"),
            minimize: l.pick("Minimize", "최소화", "しまう", "最小化"),
            zoom: l.pick("Zoom", "확대/축소", "拡大/縮小", "缩放"),
            close: l.pick("Close", "닫기", "閉じる", "关闭"),
            next_tab: l.pick("Show Next Tab", "다음 탭 보기", "次のタブを表示", "显示下一个标签页"),
            previous_tab: l.pick("Show Previous Tab", "이전 탭 보기", "前のタブを表示", "显示上一个标签页"),
            next_pane: l.pick("Next Pane", "다음 패널", "次のパネル", "下一个面板"),
            previous_pane: l.pick("Previous Pane", "이전 패널", "前のパネル", "上一个面板"),
            go_to_tab: l.pick("Go to Tab", "탭으로 이동", "タブへ移動", "转到标签页"),
            last_tab: l.pick("Last Tab", "마지막 탭", "最後のタブ", "最后一个标签页"),
            language: l,
        }
    }

    pub fn tab_number(&self, number: u8) -> String {
        match self.language {
            Language::En => format!("Tab {number}"),
            Language::Ko => format!("탭 {number}"),
            Language::Ja => format!("タブ {number}"),
            Language::Zh => format!("标签页 {number}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_tags_pick_a_language() {
        assert_eq!(Language::from_tag("ko-KR"), Language::Ko);
        assert_eq!(Language::from_tag("ja"), Language::Ja);
        assert_eq!(Language::from_tag("zh-Hans-CN"), Language::Zh);
        assert_eq!(Language::from_tag("ZH_TW"), Language::Zh);
        assert_eq!(Language::from_tag("de-DE"), Language::En);
        assert_eq!(Language::from_tag(""), Language::En);
    }

    #[test]
    fn tab_numbers_read_in_each_language() {
        assert_eq!(MenuText::new(Language::Ko).tab_number(3), "탭 3");
        assert_eq!(MenuText::new(Language::En).tab_number(1), "Tab 1");
    }
}
