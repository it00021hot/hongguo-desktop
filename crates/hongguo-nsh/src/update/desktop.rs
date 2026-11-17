use nsh_core::prelude::*;
use nsh_updater::{Available, Config, Download};
use std::path::PathBuf;

const MANIFEST: &str =
    "https://github.com/it00021hot/hongguo-desktop/releases/latest/download/guava-update.json";

pub(super) async fn check() -> Result<Option<Available>, String> {
    let mut config = Config::default();
    config.id = crate::metadata().id.clone();
    config.version = crate::metadata().version.clone();
    config.github = Some("it00021hot/hongguo-desktop".into());
    config.validate().map_err(|error| error.to_string())?;
    let manifest = get(MANIFEST).await?;
    config
        .check(&manifest, None, &nsh_updater::platform())
        .map_err(|error| error.to_string())
}

pub(super) async fn download(update: Available) -> Result<PathBuf, String> {
    let hash = update
        .sha256
        .as_deref()
        .ok_or("update archive has no SHA-256")?;
    let mut verification = Download::new(hash, update.size);
    let bytes = get(&update.url).await?;
    verification
        .update(&bytes)
        .map_err(|error| error.to_string())?;
    verification.finish().map_err(|error| error.to_string())?;
    blocking(move || {
        use std::io::Write;
        let directory = hongguo_store::paths::data_dir().join("updates");
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        let path = directory.join(format!("{}.archive", uuid::Uuid::new_v4()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| error.to_string())?;
        if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
            let _ = std::fs::remove_file(&path);
            return Err(error.to_string());
        }
        Ok(path)
    })
    .await
}

pub(super) async fn install(path: PathBuf, update: Available) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        blocking(move || {
            use std::io::Read;
            let hash = update
                .sha256
                .as_deref()
                .ok_or("update archive has no SHA-256")?;
            let mut verification = Download::new(hash, update.size);
            let mut file = std::fs::File::open(&path).map_err(|error| error.to_string())?;
            const VERIFY_CHUNK_BYTES: usize = 64 * 1024;
            let mut chunk = [0; VERIFY_CHUNK_BYTES];
            loop {
                let size = file.read(&mut chunk).map_err(|error| error.to_string())?;
                if size == 0 {
                    break;
                }
                verification
                    .update(&chunk[..size])
                    .map_err(|error| error.to_string())?;
            }
            verification.finish().map_err(|error| error.to_string())?;
            let executable = std::env::current_exe().map_err(|error| error.to_string())?;
            let parent = executable.parent().ok_or("missing executable directory")?;
            #[cfg(target_os = "macos")]
            let resources = parent.join("../Resources/nsh");
            #[cfg(target_os = "windows")]
            let resources = parent.join("nsh");
            let config = std::fs::read(resources.join("assets").join(nsh_updater::CONFIG_ASSET))
                .map_err(|error| error.to_string())?;
            let installation =
                nsh_updater::install::Installation::current(&crate::metadata().id, &config)
                    .ok_or("not a packaged installation")?;
            if !installation.can_install() {
                return Err("application directory is not writable".into());
            }
            installation.remove_leftovers();
            installation
                .install(&path)
                .map_err(|error| error.to_string())?;
            if let Err(error) = std::fs::remove_file(path) {
                log::warn!("[NSH update] archive cleanup failed: {error}");
            }
            Ok(())
        })
        .await?;
        nsh_core::element::desktop::restart();
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (path, update);
        Err("automatic installation is only available on desktop".into())
    }
}

async fn get(url: &str) -> Result<Vec<u8>, String> {
    let response = fetch(
        HttpRequest::get(url).header("User-Agent", format!("guava/{}", crate::metadata().version)),
    )
    .await?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    Ok(response.body)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    hongguo_service::runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())?
}
