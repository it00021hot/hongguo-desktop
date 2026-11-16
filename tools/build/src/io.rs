use anyhow::{Context, Result, bail, ensure};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub type Environment = BTreeMap<String, String>;

pub fn text(path: impl AsRef<Path>) -> Result<String> {
    fs::read_to_string(path.as_ref()).with_context(|| format!("Read {}", path.as_ref().display()))
}

pub fn read_json<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T> {
    serde_json::from_str(&text(path.as_ref())?)
        .with_context(|| format!("Parse {}", path.as_ref().display()))
}

pub fn write(path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, data).with_context(|| format!("Write {}", path.display()))
}

pub fn write_json(path: impl AsRef<Path>, data: &impl Serialize) -> Result<()> {
    write(path, format!("{}\n", serde_json::to_string_pretty(data)?))
}

pub fn hash(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}
pub fn digest(path: impl AsRef<Path>) -> Result<String> {
    Ok(hash(fs::read(path.as_ref()).with_context(|| {
        format!("Hash {}", path.as_ref().display())
    })?))
}
pub fn json_digest(value: &impl Serialize) -> Result<String> {
    Ok(hash(serde_json::to_vec(value)?))
}
pub fn string(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().into_owned()
}
pub fn slash(path: impl AsRef<Path>) -> String {
    string(path).replace('\\', "/")
}
pub fn absolute(path: impl AsRef<Path>) -> Result<PathBuf> {
    Ok(std::path::absolute(path)?)
}

pub fn list(path: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(path)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    Ok(paths)
}

pub fn walk(path: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in list(path)? {
        ensure!(
            !string(&entry).contains(['\r', '\n']),
            "Paths cannot contain line breaks"
        );
        files.push(entry.clone());
        if fs::symlink_metadata(&entry)?.is_dir() {
            files.extend(walk(entry)?);
        }
    }
    Ok(files)
}

pub fn remove(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.is_symlink() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result<()> {
    let (from, to) = (from.as_ref(), to.as_ref());
    let meta = fs::symlink_metadata(from).with_context(|| format!("Copy {}", from.display()))?;
    if meta.is_symlink() {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        remove(to)?;
        let target = fs::read_link(from)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, to)?;
        #[cfg(windows)]
        if from.is_dir() {
            std::os::windows::fs::symlink_dir(target, to)?;
        } else {
            std::os::windows::fs::symlink_file(target, to)?;
        }
    } else if meta.is_dir() {
        fs::create_dir_all(to)?;
        for entry in list(from)? {
            copy(&entry, to.join(entry.file_name().context("File name")?))?;
        }
    } else {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(from, to)?;
    }
    Ok(())
}

pub fn command(
    program: impl AsRef<std::ffi::OsStr>,
    args: &[String],
    env: &Environment,
    cwd: Option<&Path>,
) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args).envs(env);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd
}

pub fn run(
    program: impl AsRef<std::ffi::OsStr>,
    args: &[String],
    env: &Environment,
    cwd: Option<&Path>,
) -> Result<()> {
    let status = command(program.as_ref(), args, env, cwd)
        .status()
        .with_context(|| format!("Start {}", program.as_ref().to_string_lossy()))?;
    ensure!(
        status.success(),
        "{} exited with {status}",
        program.as_ref().to_string_lossy()
    );
    Ok(())
}

pub fn output(program: impl AsRef<std::ffi::OsStr>, args: &[String]) -> Result<String> {
    let result = command(program.as_ref(), args, &Environment::new(), None)
        .stderr(Stdio::piped())
        .output()?;
    ensure!(
        result.status.success(),
        "{}: {}",
        program.as_ref().to_string_lossy(),
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?.trim().into())
}

pub fn words(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).into()).collect()
}
pub fn required_env(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("Set {name}"))
}
pub fn rust_target() -> Result<String> {
    if let Ok(target) = env::var("CARGO_BUILD_TARGET") {
        return Ok(target);
    }
    output("rustc", &words(&["-vV"]))?
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(str::to_owned))
        .context("rustc -vV has no host target")
}
pub fn arg(args: &[String], index: usize) -> Result<&str> {
    args.get(index)
        .map(String::as_str)
        .with_context(|| format!("Missing argument {}", index + 1))
}
pub fn options(args: &[String], allowed: &[&str]) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    for pair in args.chunks(2) {
        ensure!(
            pair.len() == 2 && allowed.contains(&pair[0].as_str()),
            "Expected options: {}",
            allowed.join(", ")
        );
        ensure!(
            result.insert(pair[0].clone(), pair[1].clone()).is_none(),
            "Duplicate option {}",
            pair[0]
        );
    }
    Ok(result)
}
pub fn option<'a>(values: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str> {
    values
        .get(name)
        .map(String::as_str)
        .with_context(|| format!("Missing {name}"))
}

pub fn relative(path: &str) -> Result<&Path> {
    ensure!(
        !path.is_empty()
            && !path.contains(['\\', ':'])
            && path
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != ".."),
        "Path escapes its root: {path}"
    );
    let result = Path::new(path);
    ensure!(!result.is_absolute(), "Absolute artifact path: {path}");
    Ok(result)
}

pub fn checked(root: &Path, path: &str) -> Result<PathBuf> {
    let file = root.join(relative(path)?).canonicalize()?;
    ensure!(
        file.starts_with(root.canonicalize()?) && file.is_file(),
        "Artifact escapes its root: {path}"
    );
    Ok(file)
}

pub fn tool(name: &str) -> PathBuf {
    if let Some(path) = env::var_os("HONGGUO_INSPECTION_LLVM_BIN") {
        return PathBuf::from(path).join(name);
    }
    PathBuf::from(name)
}

pub fn find_tool(name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() || path.components().count() > 1 {
        ensure!(path.is_file(), "Tool is missing: {name}");
        return absolute(path);
    }
    let executable = if cfg!(windows) && path.extension().is_none() {
        format!("{name}{}", env::consts::EXE_SUFFIX)
    } else {
        name.into()
    };
    env::split_paths(&env::var_os("PATH").context("PATH is not set")?)
        .map(|directory| directory.join(&executable))
        .find(|candidate| candidate.is_file())
        .with_context(|| format!("Tool {name} was not found on PATH"))
        .and_then(absolute)
}

pub fn system_tool(name: &str) -> Result<PathBuf> {
    if cfg!(windows) && ["curl", "tar"].contains(&name) {
        Ok(PathBuf::from(required_env("SystemRoot")?)
            .join("System32")
            .join(format!("{name}.exe")))
    } else {
        Ok(name.into())
    }
}

pub fn download(url: &str, path: &Path) -> Result<()> {
    if !url.starts_with("https://") {
        bail!("Downloads require HTTPS");
    }
    run(
        system_tool("curl")?,
        &words(&[
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--retry",
            "2",
            "--max-time",
            "180",
            "--output",
            &string(path),
            url,
        ]),
        &Environment::new(),
        None,
    )
}
