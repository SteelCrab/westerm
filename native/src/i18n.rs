//! Text the host writes for the user, in the dashboard's UI language.
//! Keep the languages in step with src/i18n/messages.ts.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    Ko,
    #[default]
    En,
    Ja,
    Zh,
}

impl Lang {
    /// A missing or unknown value is English.
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("ko") => Lang::Ko,
            Some("ja") => Lang::Ja,
            Some("zh") => Lang::Zh,
            _ => Lang::En,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Lang::Ko => "ko",
            Lang::En => "en",
            Lang::Ja => "ja",
            Lang::Zh => "zh",
        }
    }

    /// Line between a saved screen and the new process that replaces it.
    pub fn restored(self, saved: &str) -> String {
        match self {
            Lang::Ko => format!("이전 세션 복원 · {saved} 저장 · 새 프로세스로 시작합니다"),
            Lang::En => format!("Previous session restored · saved {saved} · running a new process"),
            Lang::Ja => format!("前回のセッションを復元 · {saved} に保存 · 新しいプロセスで開始します"),
            Lang::Zh => format!("已恢复上次会话 · 保存于 {saved} · 已启动新进程"),
        }
    }

    pub fn too_many_sessions(self, limit: usize) -> String {
        match self {
            Lang::Ko => format!("세션은 최대 {limit}개까지 열 수 있습니다"),
            Lang::En => format!("You can open at most {limit} sessions"),
            Lang::Ja => format!("開けるセッションは最大 {limit} 個です"),
            Lang::Zh => format!("最多只能打开 {limit} 个会话"),
        }
    }

    pub fn start_failed(self, error: &str) -> String {
        match self {
            Lang::Ko => format!("세션을 시작하지 못했습니다: {error}"),
            Lang::En => format!("Could not start the session: {error}"),
            Lang::Ja => format!("セッションを開始できませんでした: {error}"),
            Lang::Zh => format!("无法启动会话：{error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_languages_fall_back_to_english() {
        for lang in [Lang::Ko, Lang::En, Lang::Ja, Lang::Zh] {
            assert_eq!(Lang::parse(Some(lang.as_str())), lang);
        }
        assert_eq!(Lang::parse(Some("fr")), Lang::En);
        assert_eq!(Lang::parse(None), Lang::En);
    }
}
