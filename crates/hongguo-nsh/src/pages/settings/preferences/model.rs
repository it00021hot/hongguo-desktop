use hongguo_common::model::{ProxyConfig, Settings, settings::NamingTemplate};

const MAX_CACHE_MB: u32 = 1024 * 1024;
const MAX_CACHE_DAYS: u32 = 365;
const MAX_CONCURRENCY: u32 = 10;

#[derive(Debug)]
pub(super) struct InvalidNumber {
    pub(super) field: &'static str,
    pub(super) minimum: u32,
    pub(super) maximum: u32,
}

#[derive(Clone, PartialEq)]
pub(super) struct Draft {
    pub(super) download_dir: String,
    pub(super) cache_dir: String,
    pub(super) concurrency: String,
    pub(super) cache_limit: String,
    pub(super) cache_ttl: String,
    pub(super) naming: NamingTemplate,
    pub(super) proxy: ProxyConfig,
    pub(super) auto_next: bool,
    pub(super) pause_on_minimize: bool,
    pub(super) auto_delete: bool,
    pub(super) boss_key: String,
    initial_boss_key: String,
}

impl From<Settings> for Draft {
    fn from(settings: Settings) -> Self {
        Self {
            download_dir: settings.download_dir,
            cache_dir: settings.cache_dir,
            concurrency: settings.max_concurrency.to_string(),
            cache_limit: settings.cache_limit_mb.to_string(),
            cache_ttl: settings.cache_ttl_days.to_string(),
            naming: settings.naming,
            proxy: settings.proxy,
            auto_next: settings.auto_next_episode,
            pause_on_minimize: settings.pause_on_minimize,
            auto_delete: settings.auto_delete_after_play,
            boss_key: settings.boss_key.clone(),
            initial_boss_key: settings.boss_key,
        }
    }
}

impl Draft {
    pub(super) fn apply(
        self,
        mut latest: Settings,
        desktop: bool,
    ) -> Result<Settings, InvalidNumber> {
        latest.max_concurrency = number(
            &self.concurrency,
            1,
            MAX_CONCURRENCY,
            "settings.concurrency",
        )? as usize;
        latest.cache_limit_mb = number(&self.cache_limit, 0, MAX_CACHE_MB, "settings.cacheLimit")?;
        latest.cache_ttl_days = number(&self.cache_ttl, 0, MAX_CACHE_DAYS, "settings.cacheTtl")?;
        if desktop {
            latest.download_dir = self.download_dir;
            latest.cache_dir = self.cache_dir;
            latest.pause_on_minimize = self.pause_on_minimize;
            if self.boss_key != self.initial_boss_key {
                latest.boss_key = self.boss_key;
            }
        }
        latest.naming = self.naming;
        latest.proxy = self.proxy;
        latest.auto_next_episode = self.auto_next;
        latest.auto_delete_after_play = self.auto_delete;
        Ok(latest)
    }
}

fn number(
    value: &str,
    minimum: u32,
    maximum: u32,
    field: &'static str,
) -> Result<u32, InvalidNumber> {
    let value = value
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|number| (minimum..=maximum).contains(number));
    value.ok_or(InvalidNumber {
        field,
        minimum,
        maximum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_common::model::AccountState;

    #[test]
    fn boss_key_draft_applies_desktop_edits_and_preserves_untouched_or_mobile_values() {
        let opened = Settings {
            boss_key: "CommandOrControl+Alt+A".into(),
            ..Settings::default()
        };
        let mut draft = Draft::from(opened.clone());
        draft.boss_key = "CommandOrControl+Alt+H".into();
        assert_eq!(
            draft.clone().apply(opened.clone(), true).unwrap().boss_key,
            "CommandOrControl+Alt+H"
        );
        assert_eq!(
            draft.clone().apply(opened.clone(), false).unwrap().boss_key,
            opened.boss_key
        );
        draft.boss_key.clear();
        assert_eq!(draft.apply(opened.clone(), true).unwrap().boss_key, "");
        let latest = Settings {
            boss_key: "CommandOrControl+Alt+Z".into(),
            ..opened.clone()
        };
        assert_eq!(
            Draft::from(opened)
                .apply(latest.clone(), true)
                .unwrap()
                .boss_key,
            latest.boss_key
        );
    }

    #[test]
    fn saving_an_older_draft_preserves_the_toolbar_theme() {
        let opened = Settings {
            theme: "dark".into(),
            ..Settings::default()
        };
        let draft = Draft::from(opened);
        let latest = Settings {
            theme: "light".into(),
            ..Settings::default()
        };
        let saved = draft.apply(latest, true).unwrap();
        assert_eq!(saved.theme, "light");
    }

    #[test]
    fn saving_preferences_preserves_the_latest_account_and_shell_fields() {
        let mut draft = Draft::from(Settings::default());
        draft.concurrency = "7".into();
        let latest = Settings {
            boss_key: "CommandOrControl+Alt+H".into(),
            account: Some(AccountState {
                mobile: String::new(),
                cookies: String::new(),
                user_name: "new login".into(),
                avatar_url: String::new(),
                user_id: "42".into(),
                login_at: 1,
                token: String::new(),
                raw_login: String::new(),
                raw_profile: String::new(),
            }),
            ..Settings::default()
        };
        let saved = draft.apply(latest.clone(), true).unwrap();
        assert_eq!(saved.max_concurrency, 7);
        assert_eq!(saved.account, latest.account);
        assert_eq!(saved.boss_key, latest.boss_key);
    }

    #[test]
    fn mobile_preferences_preserve_the_private_directories() {
        let mut draft = Draft::from(Settings::default());
        draft.download_dir = "/unavailable".into();
        draft.cache_dir = "/unavailable".into();
        let latest = Settings {
            download_dir: "/private/downloads".into(),
            cache_dir: "/private/cache".into(),
            ..Settings::default()
        };
        let saved = draft.apply(latest.clone(), false).unwrap();
        assert_eq!(saved.download_dir, latest.download_dir);
        assert_eq!(saved.cache_dir, latest.cache_dir);
    }

    #[test]
    fn invalid_numeric_preferences_are_rejected_without_silent_clamping() {
        for value in ["", "-1", "1.5", "x", "0", "11", "999999999999999999999"] {
            let mut draft = Draft::from(Settings::default());
            draft.concurrency = value.into();
            assert!(draft.apply(Settings::default(), true).is_err(), "{value}");
        }
        for value in ["", "-1", "1.5", "x", "1048577"] {
            let mut draft = Draft::from(Settings::default());
            draft.cache_limit = value.into();
            assert!(draft.apply(Settings::default(), true).is_err(), "{value}");
        }
        for value in ["", "-1", "366"] {
            let mut draft = Draft::from(Settings::default());
            draft.cache_ttl = value.into();
            assert!(draft.apply(Settings::default(), true).is_err(), "{value}");
        }
    }

    #[test]
    fn numeric_boundaries_and_whitespace_are_accepted() {
        for (concurrency, limit, ttl) in [("1", "0", "0"), (" 10 ", "1048576", "365")] {
            let mut draft = Draft::from(Settings::default());
            draft.concurrency = concurrency.into();
            draft.cache_limit = limit.into();
            draft.cache_ttl = ttl.into();
            let saved = draft.apply(Settings::default(), true).unwrap();
            assert_eq!(
                saved.max_concurrency,
                concurrency.trim().parse::<usize>().unwrap()
            );
            assert_eq!(saved.cache_limit_mb, limit.parse::<u32>().unwrap());
            assert_eq!(saved.cache_ttl_days, ttl.parse::<u32>().unwrap());
        }
    }
}
