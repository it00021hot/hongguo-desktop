use crate::{io::*, media};
use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde_json::{Value, json};
use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

struct Target {
    triple: &'static str,
    platform: &'static str,
    os: &'static str,
    arch: &'static str,
}
const TARGETS: &[Target] = &[
    Target {
        triple: "x86_64-pc-windows-msvc",
        platform: "windows-x86_64",
        os: "windows",
        arch: "x86_64",
    },
    Target {
        triple: "aarch64-apple-darwin",
        platform: "macos-aarch64",
        os: "macos",
        arch: "arm64",
    },
    Target {
        triple: "x86_64-apple-darwin",
        platform: "macos-x86_64",
        os: "macos",
        arch: "x86_64",
    },
];

pub fn preflight(args: &[String]) -> Result<()> {
    let values = options(args, &["--tag", "--notes", "--artifacts"])?;
    let app: Value = read_json("crates/hongguo-nsh/nsh.json")?;
    let version = app["version"].as_str().context("NSH version missing")?;
    ensure!(
        Regex::new(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")?.is_match(version)
            && app["build"].as_u64().is_some_and(|v| v >= 1),
        "NSH version/build is invalid"
    );
    let metadata: Value = serde_json::from_str(&output(
        "cargo",
        &words(&["metadata", "--locked", "--no-deps", "--format-version", "1"]),
    )?)?;
    let members = metadata["workspace_members"]
        .as_array()
        .context("Workspace members")?;
    let lock = text("Cargo.lock")?.replace("\r\n", "\n");
    for package in metadata["packages"]
        .as_array()
        .context("Cargo packages")?
        .iter()
        .filter(|p| members.contains(&p["id"]))
    {
        let name = package["name"].as_str().context("Package name")?;
        ensure!(
            package["version"] == version,
            "Version mismatch: {name}={}, NSH={version}",
            package["version"]
        );
        ensure!(
            lock.contains(&format!(
                "[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n"
            )),
            "Cargo.lock is not synchronized for {name}"
        );
    }
    let changelog = text("CHANGELOG.md")?;
    let section = changelog
        .split("\n## ")
        .find(|s| s.starts_with(&format!("[{version}] - ")))
        .context("Missing version changelog")?;
    ensure!(section.contains("\n- "), "Empty version changelog");
    let sha = output("git", &words(&["rev-parse", "HEAD"]))?;
    if let Some(tag) = values.get("--tag") {
        ensure!(
            *tag == format!("v{version}"),
            "Expected release tag v{version}"
        );
        let tagged = output(
            "git",
            &words(&[
                "rev-parse",
                "--verify",
                &format!("refs/tags/{tag}^{{commit}}"),
            ]),
        )?;
        ensure!(
            tagged == sha,
            "Release tag does not point to checked-out commit"
        );
    }
    if let Some(notes) = values.get("--notes") {
        write(
            notes,
            format!(
                "## {}\n\nWindows: unsigned Authenticode. macOS: ad hoc signed, not notarized.\n",
                section.trim()
            ),
        )?;
    }
    if let Some(directory) = values.get("--artifacts") {
        verify_artifacts(Path::new(directory), &app, &sha)?;
        update_manifest(Path::new(directory), &app, section)?;
    }
    if let Some(path) = env::var_os("GITHUB_OUTPUT") {
        writeln!(
            OpenOptions::new().append(true).open(path)?,
            "version={version}\nsha={sha}\ntag=v{version}"
        )?;
    }
    println!(
        "Release metadata verified: v{version}, build {}, {sha}",
        app["build"]
    );
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateManifest {
    id: String,
    version: String,
    notes: Option<String>,
    platforms: std::collections::BTreeMap<String, UpdateAsset>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAsset {
    url: String,
    sha256: Option<String>,
    size: Option<u64>,
}

fn update_manifest(directory: &Path, app: &Value, notes: &str) -> Result<()> {
    let mut manifest = UpdateManifest {
        id: app["id"].as_str().context("App id")?.into(),
        version: app["version"].as_str().context("App version")?.into(),
        notes: Some(notes.trim().into()),
        platforms: std::collections::BTreeMap::new(),
    };
    for target in TARGETS {
        let stem = format!("guava-{}-{}", manifest.version, target.platform);
        let filename = if target.os == "windows" {
            format!("{stem}-portable.zip")
        } else {
            format!("{stem}.tar.gz")
        };
        let archive = directory.join(&filename);
        let asset = UpdateAsset {
            url: filename,
            sha256: Some(digest(&archive)?),
            size: Some(fs::metadata(&archive)?.len()),
        };
        manifest.platforms.insert(target.platform.into(), asset);
    }
    write(
        directory.join("guava-update.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )
}

fn artifact_names(stem: &str, windows: bool) -> Vec<String> {
    if windows {
        vec![format!("{stem}-setup.exe"), format!("{stem}-portable.zip")]
    } else {
        vec![format!("{stem}.tar.gz"), format!("{stem}.dmg")]
    }
}

fn checksums(directory: &Path, names: &[String]) -> Result<String> {
    let mut result = String::new();
    for name in names {
        let path = directory.join(relative(name)?);
        ensure!(fs::metadata(&path)?.len() > 0, "Empty artifact: {name}");
        result.push_str(&format!("{}  {name}\n", digest(path)?));
    }
    Ok(result)
}

fn verify_artifacts(directory: &Path, app: &Value, sha: &str) -> Result<()> {
    for target in TARGETS {
        let stem = format!(
            "{}-{}-{}",
            app["name"].as_str().context("App name")?,
            app["version"].as_str().context("App version")?,
            target.platform
        );
        let manifest_name = format!("{stem}.json");
        let manifest: Value = read_json(directory.join(&manifest_name))?;
        ensure!(
            manifest["commit"] == sha
                && manifest["dirty"] == false
                && manifest["version"] == app["version"]
                && manifest["build"] == app["build"]
                && manifest["id"] == app["id"]
                && manifest["triple"] == target.triple
                && manifest["signing"]
                    == if target.os == "windows" {
                        "unsigned"
                    } else {
                        "ad-hoc-not-notarized"
                    },
            "Artifact provenance mismatch: {}",
            target.platform
        );
        let mut expected = artifact_names(&stem, target.os == "windows");
        ensure!(
            manifest["artifacts"] == serde_json::to_value(&expected)?,
            "Artifact list mismatch: {}",
            target.platform
        );
        expected.push(manifest_name);
        ensure!(
            text(directory.join(format!("SHA256SUMS-{}.txt", target.platform)))?
                == checksums(directory, &expected)?,
            "Artifact checksum mismatch: {}",
            target.platform
        );
    }
    Ok(())
}

pub fn assets(args: &[String]) -> Result<()> {
    let triple = arg(args, 0)?;
    let target = TARGETS
        .iter()
        .find(|t| t.triple == triple)
        .context("Unsupported desktop release target")?;
    // Windows 审计是纯文件解析，交叉宿主可执行；macOS 审计依赖 PlistBuddy、
    // ditto、hdiutil 与 codesign，只能在 macOS 上运行。
    ensure!(
        env::consts::OS == target.os || target.os == "windows",
        "macOS release artifacts require their native runner"
    );
    let source = absolute(arg(args, 1)?)?;
    let destination = absolute(arg(args, 2)?)?;
    fs::create_dir_all(&destination)?;
    let app: Value = read_json("crates/hongguo-nsh/nsh.json")?;
    let name = app["name"].as_str().context("App name")?;
    let version = app["version"].as_str().context("App version")?;
    let stem = format!("{name}-{version}-{}", target.platform);
    let mut artifacts = artifact_names(&stem, target.os == "windows");
    if target.os == "windows" {
        media::audit_windows(&source.join(name))?;
        let packaged: Value = read_json(source.join(name).join("nsh/app.json"))?;
        ensure!(
            packaged["version"] == app["version"] && packaged["id"] == app["id"],
            "Packaged Windows metadata mismatch"
        );
        let installer = text(source.join(format!("{name}.iss")))?;
        let display = app["windows"]["displayName"]
            .as_str()
            .context("Installer display name")?;
        let language = app["windows"]["installerLanguage"]
            .as_str()
            .context("Installer language")?;
        for expected in [
            format!("AppId={}", app["id"].as_str().context("App ID")?),
            format!("UninstallDisplayName={display}"),
            format!("{{autoprograms}}\\{display}"),
            format!("{{autodesktop}}\\{display}"),
            format!("Languages\\{language}.isl"),
        ] {
            ensure!(
                installer.contains(&expected),
                "Installer contract missing: {expected}"
            );
        }
        for (from, to) in [
            (format!("{name}-setup.exe"), &artifacts[0]),
            (format!("{name}.zip"), &artifacts[1]),
        ] {
            copy(source.join(from), destination.join(to))?;
        }
    } else {
        let bundle = source.join(format!("{name}.app"));
        media::audit_macos(&bundle)?;
        let plist = bundle.join("Contents/Info.plist");
        for (key, value) in [
            (
                "CFBundleIdentifier",
                app["id"].as_str().context("App ID")?.to_owned(),
            ),
            ("CFBundleShortVersionString", version.into()),
            ("CFBundleVersion", app["build"].to_string()),
        ] {
            ensure!(
                output(
                    "/usr/libexec/PlistBuddy",
                    &words(&["-c", &format!("Print :{key}"), &string(&plist)])
                )? == value,
                "Packaged macOS metadata mismatch: {key}"
            );
        }
        run(
            "codesign",
            &words(&["--verify", "--deep", "--strict", &string(&bundle)]),
            &Environment::new(),
            None,
        )?;
        ensure!(
            output(
                "lipo",
                &words(&["-archs", &string(bundle.join("Contents/MacOS").join(name))])
            )? == target.arch,
            "macOS artifact architecture mismatch"
        );
        let env = Environment::from([("COPYFILE_DISABLE".into(), "1".into())]);
        run(
            system_tool("tar")?,
            &words(&[
                "-czf",
                &string(destination.join(&artifacts[0])),
                "-C",
                &string(&source),
                &format!("{name}.app"),
            ]),
            &env,
            None,
        )?;
        disk_image(&bundle, &destination.join(&artifacts[1]), &app)?;
    }
    let manifest = format!("{stem}.json");
    write_json(
        destination.join(&manifest),
        &json!({"version":app["version"],"build":app["build"],"id":app["id"],"triple":triple,"commit":output("git", &words(&["rev-parse","HEAD"]))?,"dirty":!output("git", &words(&["status","--porcelain","--untracked-files=no"]))?.is_empty(),"signing":if target.os == "windows" { "unsigned" } else { "ad-hoc-not-notarized" },"artifacts":artifacts}),
    )?;
    artifacts.push(manifest);
    write(
        destination.join(format!("SHA256SUMS-{}.txt", target.platform)),
        checksums(&destination, &artifacts)?,
    )?;
    println!("Prepared {}: {}", target.platform, artifacts.join(", "));
    Ok(())
}

fn disk_image(bundle: &Path, output: &Path, app: &Value) -> Result<()> {
    let staging = tempfile::Builder::new().prefix("guava-dmg-").tempdir()?;
    let staged = staging
        .path()
        .join(format!("{}.app", app["name"].as_str().context("App name")?));
    run(
        "ditto",
        &words(&[&string(bundle), &string(&staged)]),
        &Environment::new(),
        None,
    )?;
    run(
        "codesign",
        &words(&["--verify", "--deep", "--strict", &string(staged)]),
        &Environment::new(),
        None,
    )?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("/Applications", staging.path().join("Applications"))?;
    run(
        "hdiutil",
        &words(&[
            "create",
            "-volname",
            app["nativeHost"]["displayNames"]["zh-CN"]
                .as_str()
                .context("Volume name")?,
            "-srcfolder",
            &string(staging.path()),
            "-format",
            "UDZO",
            "-ov",
            &string(output),
        ]),
        &Environment::new(),
        None,
    )?;
    run(
        "hdiutil",
        &words(&["verify", &string(output)]),
        &Environment::new(),
        None,
    )
}
