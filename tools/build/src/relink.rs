use crate::io::*;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

const MAX_RESPONSE_DEPTH: usize = 8;
const RESPONSE_THRESHOLD: usize = 16_000;
const SEARCH_HASH_LENGTH: usize = 16;

pub fn environment(
    triple: &str,
    prefix: &Path,
    linker: &str,
    sdk: &str,
    destination: &Path,
    args: &[String],
) -> Result<Environment> {
    let tools = absolute("target/native-build-tools")?;
    fs::create_dir_all(&tools)?;
    let alias = tools.join(format!("native-media-link{}", env::consts::EXE_SUFFIX));
    copy(env::current_exe()?, &alias)?;
    Ok(Environment::from([
        ("HONGGUO_HEVC_ROOT".into(), string(prefix)),
        ("HONGGUO_RELINK_REAL_LINKER".into(), linker.into()),
        (
            "HONGGUO_RELINK_REAL_LINKER_ARGS".into(),
            serde_json::to_string(args)?,
        ),
        ("HONGGUO_RELINK_SDK_ROOT".into(), sdk.into()),
        (
            "HONGGUO_RELINK_OUTPUT".into(),
            string(absolute(destination)?),
        ),
        ("HONGGUO_RELINK_TARGET".into(), triple.into()),
        (
            format!(
                "CARGO_TARGET_{}_LINKER",
                triple.replace('-', "_").to_uppercase()
            ),
            string(alias),
        ),
    ]))
}

fn response_arguments(text: &str) -> Result<Vec<String>> {
    let mut values = Vec::new();
    let (mut value, mut quote, mut escaped, mut present) = (String::new(), None, false, false);
    for character in text.chars() {
        if escaped {
            value.push(character);
            escaped = false;
            present = true;
        } else if character == '\\' && quote != Some('\'') {
            escaped = true;
        } else if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            } else {
                value.push(character);
            }
            present = true;
        } else if character == '"' || character == '\'' {
            quote = Some(character);
            present = true;
        } else if character.is_whitespace() {
            if present {
                values.push(std::mem::take(&mut value));
                present = false;
            }
        } else {
            value.push(character);
            present = true;
        }
    }
    ensure!(
        quote.is_none() && !escaped,
        "Invalid linker response file quoting"
    );
    if present {
        values.push(value);
    }
    Ok(values)
}

fn expand_response(args: &[String], depth: usize) -> Result<Vec<String>> {
    ensure!(
        depth <= MAX_RESPONSE_DEPTH,
        "Linker response nesting exceeds eight levels"
    );
    let mut result = Vec::new();
    for arg in args {
        match arg.strip_prefix('@').filter(|p| Path::new(p).exists()) {
            Some(path) => result.extend(expand_response(
                &response_arguments(&text(path)?)?,
                depth + 1,
            )?),
            None => result.push(arg.clone()),
        }
    }
    Ok(result)
}

fn execute(linker: &str, args: &[String], env: &Environment) -> Result<()> {
    if cfg!(windows) || args.iter().map(String::len).sum::<usize>() > RESPONSE_THRESHOLD {
        let temporary = tempfile::Builder::new()
            .prefix("hongguo-link-arguments-")
            .tempdir()?;
        let response = temporary.path().join("link.args");
        let contents = args
            .iter()
            .map(|a| {
                serde_json::to_string(&if cfg!(windows) {
                    a.replace('\\', "/")
                } else {
                    a.clone()
                })
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        write(&response, contents)?;
        run(linker, &[format!("@{}", response.display())], env, None)
    } else {
        run(linker, args, env, None)
    }
}

struct Capture<'a> {
    staging: &'a Path,
    sdk: PathBuf,
    prefix: PathBuf,
    toolchain: Option<PathBuf>,
    files: Vec<crate::sdk::Artifact>,
    captured: HashSet<String>,
}

impl Capture<'_> {
    fn save(&mut self, path: &Path, name: Option<&str>) -> Result<String> {
        let sha256 = digest(path)?;
        let local = match name {
            Some(name) => name.into(),
            None => format!(
                "objects/{sha256}-{}",
                path.file_name()
                    .context("Link input filename")?
                    .to_string_lossy()
            ),
        };
        if self.captured.insert(local.clone()) {
            copy(path, self.staging.join(&local))?;
            self.files.push(crate::sdk::Artifact {
                path: local.clone(),
                sha256,
                byte_length: fs::metadata(path)?.len(),
            });
        }
        Ok(format!("{{MATERIALS}}/{local}"))
    }

    fn rewrite_path(&mut self, path: &str, search: bool) -> Result<String> {
        let absolute = absolute(path)?;
        for (parent, token) in [
            (Some(&self.sdk), "{SDKROOT}"),
            (Some(&self.prefix), "{HEVC_PREFIX}"),
            (self.toolchain.as_ref(), "{TOOLCHAIN_ROOT}"),
        ] {
            if let Some(child) = parent.and_then(|p| absolute.strip_prefix(p).ok()) {
                return Ok(if child.as_os_str().is_empty() {
                    token.into()
                } else {
                    format!("{token}/{}", slash(child))
                });
            }
        }
        if !absolute.exists() {
            return Ok(path.into());
        }
        if absolute.is_file() {
            return self.save(&absolute, None);
        }
        if search {
            let directory = format!("search/{}", &hash(string(&absolute))[..SEARCH_HASH_LENGTH]);
            fs::create_dir_all(self.staging.join(&directory))?;
            for entry in list(&absolute)? {
                if entry.is_file()
                    && entry
                        .extension()
                        .is_some_and(|s| ["a", "so", "dylib"].iter().any(|ext| s == *ext))
                {
                    let name = format!(
                        "{directory}/{}",
                        entry
                            .file_name()
                            .context("Search input filename")?
                            .to_string_lossy()
                    );
                    self.save(&entry, Some(&name))?;
                }
            }
            return Ok(format!("{{MATERIALS}}/{directory}"));
        }
        Ok(path.into())
    }

    fn rewrite(&mut self, value: &str, search: bool) -> Result<String> {
        if Path::new(value).is_absolute() || Path::new(value).exists() {
            return self.rewrite_path(value, search);
        }
        if let Some(value) = value.strip_prefix("-Wl,") {
            let parts = value
                .split(',')
                .map(|p| self.rewrite(p, false))
                .collect::<Result<Vec<_>>>()?;
            return Ok(format!("-Wl,{}", parts.join(",")));
        }
        if let Some((key, path)) = value.split_once('=')
            && Path::new(path).is_absolute()
        {
            return Ok(format!("{key}={}", self.rewrite_path(path, search)?));
        }
        if (value.starts_with("-L") || value.starts_with("-F"))
            && Path::new(&value[2..]).is_absolute()
        {
            return Ok(format!(
                "{}{}",
                &value[..2],
                self.rewrite_path(&value[2..], true)?
            ));
        }
        Ok(value.into())
    }
}

pub fn capture(args: &[String]) -> Result<()> {
    let linker = required_env("HONGGUO_RELINK_REAL_LINKER")?;
    let destination = absolute(required_env("HONGGUO_RELINK_OUTPUT")?)?;
    let sdk = required_env("HONGGUO_RELINK_SDK_ROOT")?;
    let prefix = absolute(required_env("HONGGUO_HEVC_ROOT")?)?;
    let triple = required_env("HONGGUO_RELINK_TARGET")?;
    let mut expanded: Vec<String> = serde_json::from_str(
        &env::var("HONGGUO_RELINK_REAL_LINKER_ARGS").unwrap_or_else(|_| "[]".into()),
    )?;
    expanded.extend(expand_response(args, 0)?);
    let output_index = expanded
        .iter()
        .position(|a| a == "-o")
        .context("Final linker invocation has no -o output")?
        + 1;
    let binary = expanded
        .get(output_index)
        .context("Missing linker output argument")?;
    let parent = destination.parent().context("Relink output parent")?;
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".relink-staging-")
        .tempdir_in(parent)?;
    let toolchain = if triple.contains("apple") {
        Some(absolute(
            Path::new(&linker)
                .parent()
                .and_then(Path::parent)
                .context("Toolchain root")?,
        )?)
    } else {
        None
    };
    let mut capture = Capture {
        staging: staging.path(),
        sdk: absolute(&sdk)?,
        prefix: prefix.clone(),
        toolchain,
        files: Vec::new(),
        captured: HashSet::new(),
    };
    capture.save(
        &prefix.join("lib/libde265.a"),
        Some("libde265/lib/libde265.a"),
    )?;
    let mut command = Vec::new();
    for (index, argument) in expanded.iter().enumerate() {
        command.push(if index == output_index {
            "{OUTPUT}".into()
        } else {
            capture.rewrite(
                argument,
                index > 0 && ["-L", "-F"].contains(&expanded[index - 1].as_str()),
            )?
        });
    }
    ensure!(
        capture.files.iter().any(|f| f.path.starts_with("objects/")),
        "Final link captured no application objects"
    );
    execute(&linker, &expanded, &Environment::new())?;
    write_json(
        staging.path().join("link.json"),
        &json!({"schemaVersion":1,"target":triple,"linker":linker,"sdk":sdk,"outputName":Path::new(binary).file_name(),"linkedOutputPath":absolute(binary)?,"outputSha256":digest(binary)?,"args":command,"files":capture.files}),
    )?;
    let previous = destination.with_extension("previous");
    remove(&previous)?;
    if destination.exists() {
        fs::rename(&destination, &previous)?;
    }
    if let Err(error) = fs::rename(staging.path(), &destination) {
        if previous.exists() {
            fs::rename(&previous, &destination).context("Restore previous relinking materials")?;
        }
        return Err(error.into());
    }
    remove(previous)
}

pub fn finish(binary: &Path, triple: &str, destination: &Path) -> Result<()> {
    let mut manifest: Value = read_json(destination.join("link.json"))?;
    let sha256 = digest(binary)?;
    ensure!(
        manifest["target"] == triple
            && digest(
                manifest["linkedOutputPath"]
                    .as_str()
                    .context("Missing linked output path")?
            )? == sha256,
        "Relinking materials do not match the application binary"
    );
    if manifest.get("linkerOutputSha256").is_none() {
        manifest["linkerOutputSha256"] = manifest["outputSha256"].clone();
    }
    manifest["outputSha256"] = sha256.clone().into();
    write_json(destination.join("link.json"), &manifest)?;
    for name in ["Cargo.toml", "Cargo.lock", "LICENSE"] {
        copy(name, destination.join("BuildSource").join(name))?;
    }
    distribute_tool(&destination.join("BuildTool"))?;
    let sysroot = output(
        env::var("RUSTC").unwrap_or_else(|_| "rustc".into()),
        &words(&["--print", "sysroot"]),
    )?;
    for name in ["COPYRIGHT-library.html", "licenses"] {
        copy(
            Path::new(&sysroot).join("share/doc/rust").join(name),
            destination.join("RustNotices").join(name),
        )?;
    }
    let notes = if triple.contains("apple-ios") {
        "Copy the supplied app to a working directory. For simulator signing, run:\n\ncargo run --target host-tuple --manifest-path BuildTool/Cargo.toml -- resign-ios --app <guava.app> --binary <new-binary> --identity -\n\nFor a device, supply --identity <authorized-identity> --profile <profile.mobileprovision>. The tool validates identifier, expiration and entitlements before replacing the executable and signing.\n"
    } else {
        "Use --output AndroidSource/target/native-runtime/lib/arm64-v8a/libnsh.so, then rebuild the supplied NSH CLI and package without recompiling the runtime:\n\ncd AndroidSource\ncargo build --manifest-path vendor/nsh/Cargo.toml -p nsh-cli --target-dir target/nsh-cli\ntarget/nsh-cli/debug/nsh package crates/hongguo-nsh --target android --abi arm64-v8a\n\nOn Windows use nsh.exe. NSH defaults to its developer debug key; use --keystore, --key-alias and NSH_KEYSTORE_PASSWORD for production signing. No private keys are distributed.\n"
    };
    write(
        destination.join("README.md"),
        format!(
            "# Replacing the statically linked libde265 library\n\nTarget: {triple}. Application SHA-256 before bundle signing: {sha256}.\n\nlink.json records actual final-link inputs; objects/ contains application/Rust/native inputs. Codec source and licenses accompany these materials in ../NativeMedia/. SDK/NDK tools and private signing keys are not redistributed.\n\nBuildTool contains the Rust tool source and dependency lock. From this Relink directory, use Rust and a compatible target compiler/SDK:\n\n```sh\ncargo run --target host-tuple --manifest-path BuildTool/Cargo.toml -- relink --materials . --linker <target-clang> --sdk <Apple-SDK-or-Android-NDK> --hevc <modified-libde265-prefix> --output <new-binary>\n```\n\nOmit --hevc to use the supplied original archive. A replacement provides ABI-compatible lib/libde265.a. Original inputs are checked by SHA-256 before linking.\n\n{notes}"
        ),
    )
}

pub fn distribute_tool(destination: &Path) -> Result<()> {
    copy("tools/build", destination)?;
    let manifest = text(destination.join("Cargo.toml"))?;
    let metadata: Value = read_json("crates/hongguo-nsh/nsh.json")?;
    let manifest = manifest
        .lines()
        .map(|line| match line {
            "version.workspace = true" => format!("version = {}", metadata["version"]),
            "edition.workspace = true" => "edition = \"2024\"".into(),
            "rust-version.workspace = true" => "rust-version = \"1.99.1\"".into(),
            "license.workspace = true" => "license = \"PolyForm-Noncommercial-1.0.0\"".into(),
            _ => line.into(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    ensure!(
        !manifest.contains(".workspace ="),
        "Build tool manifest contains unresolved workspace inheritance"
    );
    write(
        destination.join("Cargo.toml"),
        format!("{manifest}\n[workspace]\n"),
    )?;
    copy("Cargo.lock", destination.join("Cargo.lock"))?;
    output(
        "cargo",
        &words(&[
            "metadata",
            "--offline",
            "--format-version=1",
            "--manifest-path",
            &string(destination.join("Cargo.toml")),
        ]),
    )?;
    Ok(())
}

pub fn android_source(destination: &Path, output_dir: &Path) -> Result<()> {
    let snapshot = destination.join("AndroidSource");
    remove(&snapshot)?;
    let files = output(
        "git",
        &words(&[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            "Cargo.toml",
            "Cargo.lock",
            "crates",
            "assets",
            "scripts",
            "tools/build",
            "vendor/nsh",
            "vendor/wgpu-hal",
        ]),
    )?;
    for file in files.split('\0').filter(|s| !s.is_empty()) {
        if Path::new(file).exists() {
            copy(file, snapshot.join(relative(file)?))?;
        }
    }
    copy(
        output_dir.join("runtime"),
        snapshot.join("target/native-runtime"),
    )?;
    let path = snapshot.join("crates/hongguo-nsh/nsh.json");
    let mut config: Value = read_json(&path)?;
    let hook = json!([
        "cargo",
        "run",
        "--locked",
        "--target",
        "host-tuple",
        "--manifest-path",
        "../../tools/build/Cargo.toml",
        "--",
        "reuse-runtime"
    ]);
    config["nativeHost"] = json!({"displayNames": config["nativeHost"]["displayNames"], "build":hook,"stage":hook,"hosts":{"android":"../../target/native-runtime"}});
    write_json(path, &config)
}

pub fn replay(args: &[String]) -> Result<()> {
    let options = options(
        args,
        &["--materials", "--linker", "--sdk", "--hevc", "--output"],
    )?;
    let materials = absolute(option(&options, "--materials")?)?;
    let linker = option(&options, "--linker")?;
    let sdk = option(&options, "--sdk")?;
    let out = absolute(option(&options, "--output")?)?;
    let manifest: Value = read_json(materials.join("link.json"))?;
    let files: Vec<crate::sdk::Artifact> = serde_json::from_value(manifest["files"].clone())?;
    ensure!(
        manifest["schemaVersion"] == 1 && !files.is_empty(),
        "Invalid relink manifest"
    );
    for file in files {
        crate::sdk::verify_bytes(
            &checked(&materials, &file.path)?,
            &file.sha256,
            Some(file.byte_length),
        )?;
    }
    let toolchain = Path::new(linker)
        .parent()
        .and_then(Path::parent)
        .context("Target toolchain root")?;
    let prefix = options
        .get("--hevc")
        .map(PathBuf::from)
        .unwrap_or_else(|| materials.join("libde265"));
    let substitutions = [
        ("{MATERIALS}", string(&materials)),
        ("{SDKROOT}", string(absolute(sdk)?)),
        ("{TOOLCHAIN_ROOT}", string(absolute(toolchain)?)),
        ("{HEVC_PREFIX}", string(absolute(prefix)?)),
        ("{OUTPUT}", string(&out)),
    ];
    let mut command: Vec<String> = serde_json::from_value(manifest["args"].clone())?;
    for argument in &mut command {
        for (token, path) in &substitutions {
            *argument = argument.replace(token, path);
        }
    }
    fs::create_dir_all(out.parent().context("Relink output parent")?)?;
    let mut env = Environment::new();
    if manifest["target"]
        .as_str()
        .context("Relink target")?
        .contains("apple")
    {
        env.insert("SDKROOT".into(), sdk.into());
    }
    execute(linker, &command, &env)
}
