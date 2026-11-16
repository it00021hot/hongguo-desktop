//! Native hosts supplied by embedding apps, packaged and signed by NSH.
use anyhow::{Context as _, bail};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    #[serde(default, rename = "displayNames")]
    _display_names: BTreeMap<String, String>,
    build: Vec<String>,
    stage: Vec<String>,
    hosts: BTreeMap<String, String>,
}

fn read(directory: &Path) -> anyhow::Result<Option<Config>> {
    let value: serde_json::Value = serde_json::from_slice(&fs::read(directory.join("nsh.json"))?)?;
    value
        .get("nativeHost")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .context("invalid nsh.json nativeHost")
}

fn command(directory: &Path, argv: &[String], args: &[&str]) -> anyhow::Result<()> {
    let (program, arguments) = argv
        .split_first()
        .context("nativeHost command must not be empty")?;
    let status = Command::new(program)
        .args(arguments)
        .args(args)
        .current_dir(directory)
        .status()
        .with_context(|| format!("run nativeHost command {program}"))?;
    if !status.success() {
        bail!("nativeHost command failed: {status}");
    }
    Ok(())
}

pub(crate) fn runtime(
    directory: &Path,
    target: &str,
    release: bool,
    triple: Option<&str>,
) -> anyhow::Result<Option<PathBuf>> {
    let Some(config) = read(directory)? else {
        return Ok(None);
    };
    let path = config
        .hosts
        .get(target)
        .with_context(|| format!("nativeHost has no {target} host"))?;
    let profile = if release { "release" } else { "debug" };
    command(
        directory,
        &config.build,
        &[target, profile, triple.unwrap_or("")],
    )?;
    let path = directory.join(
        path.replace("{profile}", profile)
            .replace("{triple}", triple.unwrap_or("")),
    );
    if !path.exists() {
        bail!("nativeHost output is missing: {}", path.display());
    }
    Ok(Some(path))
}

pub(crate) fn assets(runtime: &Path) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    fn collect(
        directory: &Path,
        prefix: &str,
        assets: &mut Vec<(String, Vec<u8>)>,
    ) -> anyhow::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let name = format!(
                "{prefix}{}",
                entry
                    .file_name()
                    .to_str()
                    .context("non-UTF8 native asset")?
            );
            let kind = entry.file_type()?;
            if kind.is_dir() {
                collect(&entry.path(), &format!("{name}/"), assets)?;
            } else if kind.is_file() {
                assets.push((name, fs::read(entry.path())?));
            } else {
                bail!("native assets must be regular files");
            }
        }
        Ok(())
    }
    let mut assets = Vec::new();
    let directory = runtime.join("assets");
    if directory.is_dir() {
        collect(&directory, "", &mut assets)?;
    }
    Ok(assets)
}

pub(crate) fn configured(entry: &Path) -> anyhow::Result<bool> {
    Ok(read(entry.parent().context("nativeHost entry has no parent")?)?.is_some())
}

pub(crate) fn stage(
    entry: &Path,
    target: &str,
    destination: &Path,
    executable: &str,
) -> anyhow::Result<()> {
    let directory = entry.parent().context("nativeHost entry has no parent")?;
    let Some(config) = read(directory)? else {
        return Ok(());
    };
    let destination = dunce::canonicalize(destination)?;
    command(
        directory,
        &config.stage,
        &[
            target,
            destination.to_str().context("non-UTF8 package path")?,
            executable,
        ],
    )
}
