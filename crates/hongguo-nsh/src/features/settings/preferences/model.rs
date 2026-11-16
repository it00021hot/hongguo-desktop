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
        }
    }
}

impl Draft {
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
            boss_key
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
