use hongguo_common::model::{
    ProxyConfig, Settings,
    settings::{NamingTemplate, StartupMode, StartupPage},
};

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
    pub(super) startup_mode: StartupMode,
    pub(super) startup_page: StartupPage,
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
            boss_key: settings.boss_key,
            startup_mode: settings.startup_mode,
            startup_page: settings.startup_page,
        }
    }
}

impl Draft {
    pub(super) fn preserve_invalid_numbers(&mut self, previous: &Self) {
        for (value, old, maximum, minimum, field) in [
            (
                &mut self.concurrency,
                &previous.concurrency,
                MAX_CONCURRENCY,
                1,
                "settings.concurrency",
            ),
            (
                &mut self.cache_limit,
                &previous.cache_limit,
                MAX_CACHE_MB,
                0,
                "settings.cacheLimit",
            ),
            (
                &mut self.cache_ttl,
                &previous.cache_ttl,
                MAX_CACHE_DAYS,
                0,
                "settings.cacheTtl",
            ),
        ] {
            if number(value, minimum, maximum, field).is_err() {
                value.clone_from(old);
            }
        }
    }

    pub(super) fn invalid_number(&self) -> Option<InvalidNumber> {
        [
            (
                &self.concurrency,
                1,
                MAX_CONCURRENCY,
                "settings.concurrency",
            ),
            (&self.cache_limit, 0, MAX_CACHE_MB, "settings.cacheLimit"),
            (&self.cache_ttl, 0, MAX_CACHE_DAYS, "settings.cacheTtl"),
        ]
        .into_iter()
        .find_map(|(value, minimum, maximum, field)| number(value, minimum, maximum, field).err())
    }

    pub(super) fn apply(
        self,
        previous: &Self,
        mut latest: Settings,
        desktop: bool,
    ) -> Result<Settings, InvalidNumber> {
        if self.concurrency != previous.concurrency {
            latest.max_concurrency = number(
                &self.concurrency,
                1,
                MAX_CONCURRENCY,
                "settings.concurrency",
            )? as usize;
        }
        if self.cache_limit != previous.cache_limit {
            latest.cache_limit_mb =
                number(&self.cache_limit, 0, MAX_CACHE_MB, "settings.cacheLimit")?;
        }
        if self.cache_ttl != previous.cache_ttl {
            latest.cache_ttl_days =
                number(&self.cache_ttl, 0, MAX_CACHE_DAYS, "settings.cacheTtl")?;
        }
        macro_rules! changed {
            ($field:ident, $setting:ident) => {
                if self.$field != previous.$field {
                    latest.$setting = self.$field;
                }
            };
        }
        if desktop {
            changed!(download_dir, download_dir);
            changed!(cache_dir, cache_dir);
            changed!(pause_on_minimize, pause_on_minimize);
            changed!(boss_key, boss_key);
        }
        changed!(naming, naming);
        changed!(proxy, proxy);
        changed!(auto_next, auto_next_episode);
        changed!(auto_delete, auto_delete_after_play);
        changed!(startup_mode, startup_mode);
        changed!(startup_page, startup_page);
        Ok(latest)
    }

    pub(super) fn restore_changed(&mut self, previous: &Self, attempted: &Self, saved: Self) {
        macro_rules! restore {
            ($($field:ident),+ $(,)?) => {
                $(if previous.$field != attempted.$field && self.$field == attempted.$field {
                    self.$field = saved.$field;
                })+
            };
        }
        restore!(
            download_dir,
            cache_dir,
            concurrency,
            cache_limit,
            cache_ttl,
            naming,
            proxy,
            auto_next,
            pause_on_minimize,
            auto_delete,
            boss_key,
            startup_mode,
            startup_page
        );
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

    #[test]
    fn invalid_cache_edit_does_not_discard_other_settings() {
        let settings = Settings::default();
        let saved = Draft::from(settings.clone());
        let mut next = saved.clone();
        next.cache_limit.clear();
        next.cache_ttl = "7".into();
        next.auto_next = false;
        next.startup_mode = StartupMode::LastPage;
        assert_eq!(next.invalid_number().unwrap().field, "settings.cacheLimit");
        next.preserve_invalid_numbers(&saved);
        let persisted = next.apply(&saved, settings, true).unwrap();
        assert_eq!(persisted.cache_limit_mb, 0);
        assert_eq!(persisted.cache_ttl_days, 7);
        assert!(!persisted.auto_next_episode);
        assert_eq!(persisted.startup_mode, StartupMode::LastPage);
    }

    #[test]
    fn cache_edits_and_startup_choice_round_trip_without_overwriting_newer_values() {
        let original = Settings::default();
        let saved = Draft::from(original.clone());
        let mut next = saved.clone();
        next.cache_limit = "2048".into();
        next.cache_ttl = "30".into();
        next.startup_page = StartupPage::My;
        let latest = Settings {
            theme: "light".into(),
            max_concurrency: 8,
            ..original
        };
        let persisted = next.apply(&saved, latest, true).unwrap();
        let path = std::env::temp_dir().join(format!("guava-settings-{}.db", uuid::Uuid::new_v4()));
        {
            let store = hongguo_store::Store::open(&path).unwrap();
            store.save_settings(&persisted).unwrap();
        }
        let reopened = {
            let store = hongguo_store::Store::open_with_lock_retry(&path).unwrap();
            store.settings().unwrap().unwrap()
        };
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(path.with_file_name(format!(
                "{}{suffix}",
                path.file_name().unwrap().to_string_lossy()
            )));
        }
        assert_eq!(reopened.cache_limit_mb, 2048);
        assert_eq!(reopened.cache_ttl_days, 30);
        assert_eq!(reopened.startup_page, StartupPage::My);
        assert_eq!(reopened.theme, "light");
        assert_eq!(reopened.max_concurrency, 8);
    }

    #[test]
    fn upgrade_keeps_home_for_legacy_settings() {
        let row = r#"{"downloadDir":".","maxConcurrency":3}"#;
        let reopened: Settings = serde_json::from_str(row).unwrap();
        assert_eq!(reopened.startup_mode, StartupMode::Fixed);
        assert_eq!(reopened.startup_page, StartupPage::Home);
    }
}
