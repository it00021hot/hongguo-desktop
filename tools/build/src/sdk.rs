use crate::io::*;
use anyhow::{Context, Result, ensure};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

pub const SDK_VERSION: &str = "native-codecs-v2-20261114";
const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ARTIFACTS: usize = 16_384;
const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub path: String,
    pub sha256: String,
    pub byte_length: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedTarget {
    pub asset_name: String,
    pub archive_path: Option<String>,
    pub url: Option<String>,
    pub sha256: String,
    pub byte_length: u64,
    pub manifest_sha256: String,
    pub recipe_sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lock {
    pub schema_version: u32,
    pub sdk_version: String,
    pub distribution: String,
    pub targets: BTreeMap<String, LockedTarget>,
}

pub struct Sdk {
    pub manifest: Value,
    pub prefix: PathBuf,
    pub vvdec: PathBuf,
}

pub fn verify_bytes(path: &Path, sha256: &str, size: Option<u64>) -> Result<()> {
    if let Some(size) = size {
        ensure!(
            fs::metadata(path)?.len() == size,
            "Size mismatch: {}",
            path.display()
        );
    }
    ensure!(
        digest(path)? == sha256,
        "SHA-256 mismatch: {}",
        path.display()
    );
    Ok(())
}

pub fn verify(target: &str, directory: &Path) -> Result<Sdk> {
    let lock: Lock = read_json("scripts/native-sdk.sources.json")?;
    verify_locked(target, directory, &lock)
}

fn verify_locked(target: &str, directory: &Path, lock: &Lock) -> Result<Sdk> {
    ensure!(lock.schema_version == 1, "Invalid native SDK lock schema");
    let entry = lock
        .targets
        .get(target)
        .context("Unsupported native SDK target")?;
    verify_bytes(&directory.join("sdk.json"), &entry.manifest_sha256, None)?;
    let manifest: Value = read_json(directory.join("sdk.json"))?;
    ensure!(
        manifest["schemaVersion"] == 1
            && manifest["sdkVersion"] == lock.sdk_version
            && manifest["target"]["triple"] == target
            && manifest["recipeSha256"] == entry.recipe_sha256,
        "SDK target/version/recipe mismatch: {target}"
    );
    let artifacts: Vec<Artifact> = serde_json::from_value(manifest["artifacts"].clone())?;
    ensure!(
        !artifacts.is_empty() && artifacts.len() <= MAX_ARTIFACTS,
        "Invalid native SDK inventory size"
    );
    let mut names = HashSet::new();
    for item in &artifacts {
        ensure!(
            names.insert(item.path.clone()),
            "Duplicate SDK artifact: {}",
            item.path
        );
        verify_bytes(
            &checked(directory, &item.path)?,
            &item.sha256,
            Some(item.byte_length),
        )?;
    }
    let component = |name: &str| -> Result<PathBuf> {
        let path = manifest["components"][name]["manifestPath"]
            .as_str()
            .context("Missing component manifest")?;
        relative(path)?;
        ensure!(
            path.split('/').count() == 2
                && path.ends_with("/manifest.json")
                && names.contains(path),
            "Unauthenticated component manifest: {name}"
        );
        Ok(directory
            .join(path)
            .parent()
            .context("Component directory")?
            .into())
    };
    let prefix = component("hevc")?;
    let vvdec = component("vvdec")?;
    ensure!(prefix != vvdec, "SDK component roots overlap");
    Ok(Sdk {
        manifest,
        prefix,
        vvdec,
    })
}

pub fn prepare(target: &str) -> Result<PathBuf> {
    let lock: Lock = read_json("scripts/native-sdk.sources.json")?;
    ensure!(lock.schema_version == 1, "Invalid SDK lock");
    let entry = lock
        .targets
        .get(target)
        .with_context(|| format!("Unsupported SDK target: {target}"))?;
    if let Some(root) = env::var_os("HONGGUO_NATIVE_SDK_ROOT") {
        return Ok(verify_locked(target, &absolute(root)?, &lock)?.prefix);
    }
    let destination = absolute(Path::new("target/native-media").join(target))?;
    if destination.join("sdk.json").exists() {
        match verify_locked(target, &destination, &lock) {
            Ok(sdk) => return Ok(sdk.prefix),
            Err(error) => eprintln!("Native SDK cache needs replacement: {error:#}"),
        }
    }
    let parent = destination.parent().context("SDK parent")?;
    fs::create_dir_all(parent)?;
    let guard = parent.join(format!(".{target}.sdk-install.lock"));
    fs::create_dir(&guard).with_context(|| {
        format!(
            "Cannot acquire SDK install lock {}; another setup may own it",
            guard.display()
        )
    })?;
    let result = install(entry, target, &destination, &lock);
    let cleanup = fs::remove_dir(&guard);
    let installed = result?;
    cleanup?;
    Ok(installed)
}

fn install(entry: &LockedTarget, target: &str, destination: &Path, lock: &Lock) -> Result<PathBuf> {
    ensure!(
        Path::new(&entry.asset_name)
            .file_name()
            .and_then(|s| s.to_str())
            == Some(&entry.asset_name),
        "Invalid SDK asset name"
    );
    let parent = destination.parent().context("SDK parent")?;
    let staging = tempfile::Builder::new()
        .prefix(".native-sdk-stage-")
        .tempdir_in(parent)?;
    let archive = archive_for(entry, parent)?;
    println!("Preparing pinned native SDK: {target}");
    extract(&archive, target, staging.path())?;
    let staged = staging.path().join(target);
    let sdk = verify_locked(target, &staged, lock)?;
    let children = [
        sdk.prefix.file_name().context("HEVC name")?,
        sdk.vvdec.file_name().context("VVdeC name")?,
        std::ffi::OsStr::new("sdk.json"),
    ];
    let backup = tempfile::Builder::new()
        .prefix(".native-sdk-backup-")
        .tempdir_in(parent)?;
    fs::create_dir_all(destination)?;
    let mut replaced = Vec::new();
    let mut installed = Vec::new();
    let result = (|| -> Result<()> {
        for name in children {
            let path = destination.join(name);
            if path.exists() {
                fs::rename(&path, backup.path().join(name))?;
                replaced.push(name);
            }
            fs::rename(staged.join(name), path)?;
            installed.push(name);
        }
        verify_locked(target, destination, lock)?;
        Ok(())
    })();
    if let Err(error) = result {
        let rollback = (|| -> Result<()> {
            for name in installed.iter().rev() {
                remove(destination.join(name))?;
            }
            for name in replaced {
                fs::rename(backup.path().join(name), destination.join(name))?;
            }
            Ok(())
        })();
        if let Err(rollback) = rollback {
            let retained = backup.keep();
            anyhow::bail!(
                "SDK install failed: {error:#}; rollback failed: {rollback:#}; backup retained at {}",
                retained.display()
            );
        }
        return Err(error);
    }
    Ok(destination.join(sdk.prefix.file_name().context("HEVC name")?))
}

fn archive_for(entry: &LockedTarget, parent: &Path) -> Result<PathBuf> {
    let archive = if let Some(directory) = env::var_os("HONGGUO_NATIVE_SDK_ARCHIVE_DIR") {
        PathBuf::from(directory).join(&entry.asset_name)
    } else if let Some(path) = &entry.archive_path {
        let path = checked(Path::new("."), path)?;
        ensure!(
            path.starts_with(Path::new("vendor/native-codecs").canonicalize()?),
            "Vendored archive escapes its directory"
        );
        path
    } else {
        let cache = parent.join(".native-sdk-downloads");
        fs::create_dir_all(&cache)?;
        let archive = cache.join(format!("{}.tar.gz", entry.sha256));
        if !archive.exists() {
            let temporary = tempfile::Builder::new()
                .prefix(".download-")
                .tempdir_in(&cache)?;
            let path = temporary.path().join("archive.tar.gz");
            download(
                entry.url.as_deref().context("SDK download URL missing")?,
                &path,
            )?;
            verify_bytes(&path, &entry.sha256, Some(entry.byte_length))?;
            fs::rename(path, &archive)?;
        }
        ensure!(
            fs::symlink_metadata(&archive)?.is_file(),
            "SDK cache is not a regular file"
        );
        archive
    };
    verify_bytes(&archive, &entry.sha256, Some(entry.byte_length))?;
    Ok(archive)
}

fn extract(archive: &Path, target: &str, destination: &Path) -> Result<()> {
    ensure!(
        fs::metadata(archive)?.len() <= MAX_ARCHIVE_BYTES,
        "SDK archive exceeds size limit"
    );
    let mut tar = tar::Archive::new(GzDecoder::new(fs::File::open(archive)?));
    let mut total = 0_u64;
    for item in tar.entries()? {
        let mut item = item?;
        let path = slash(item.path()?);
        let path = path.trim_end_matches('/');
        relative(path)?;
        ensure!(
            path == target || path.starts_with(&format!("{target}/")),
            "Unexpected SDK target path: {path}"
        );
        total = total
            .checked_add(item.size())
            .context("Archive size overflow")?;
        ensure!(
            total <= MAX_UNPACKED_BYTES,
            "SDK unpacked size exceeds limit"
        );
        ensure!(
            item.unpack_in(destination)?,
            "Unsafe SDK archive entry: {path}"
        );
    }
    Ok(())
}

pub fn runtime_environment(prefix: &Path) -> Result<Environment> {
    let mut env = Environment::from([
        ("HONGGUO_HEVC_ROOT".into(), string(prefix)),
        (
            "HONGGUO_NATIVE_SDK_ROOT".into(),
            string(prefix.parent().context("SDK root")?),
        ),
    ]);
    let (key, folder) = if cfg!(windows) {
        ("PATH", "bin")
    } else if cfg!(target_os = "macos") {
        ("DYLD_LIBRARY_PATH", "lib")
    } else {
        return Ok(env);
    };
    let mut paths = vec![prefix.join(folder)];
    if let Some(value) = std::env::var_os(key) {
        paths.extend(std::env::split_paths(&value));
    }
    env.insert(
        key.into(),
        std::env::join_paths(paths)?.to_string_lossy().into_owned(),
    );
    Ok(env)
}

pub fn inventory(directory: &Path) -> Result<Vec<Artifact>> {
    let mut result = Vec::new();
    for path in walk(directory)? {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            continue;
        }
        if metadata.is_symlink() {
            ensure!(
                !fs::read_link(&path)?.is_absolute(),
                "SDK alias must be relative"
            );
        }
        let name = slash(path.strip_prefix(directory)?);
        let actual = checked(directory, &name)?;
        result.push(Artifact {
            path: name,
            sha256: digest(&actual)?,
            byte_length: fs::metadata(actual)?.len(),
        });
    }
    ensure!(
        result.len() <= MAX_ARTIFACTS,
        "SDK inventory exceeds file budget"
    );
    Ok(result)
}
