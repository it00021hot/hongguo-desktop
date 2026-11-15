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
