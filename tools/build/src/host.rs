use crate::{android, io::*, media, relink, sdk};
use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde_json::Value;
use std::{env, fs, path::Path};

pub fn build(args: &[String]) -> Result<()> {
    let target = arg(args, 0)?;
    let profile = args.get(1).map(String::as_str).unwrap_or("debug");
    ensure!(
        ["debug", "release"].contains(&profile),
        "Unsupported profile: {profile}"
    );
    ensure!(
        ["windows", "macos", "ios", "ios-simulator", "android"].contains(&target),
        "Unsupported host: {target}"
    );
    media::verify_workspace()?;
    if target == "android" {
        return android::build(profile);
    }
    let mobile = target.starts_with("ios");
    let requested = args.get(2).filter(|s| !s.is_empty());
    let triple = match requested {
        Some(value) => value.clone(),
        None if target == "ios" => "aarch64-apple-ios".into(),
        None if target == "ios-simulator" => "aarch64-apple-ios-sim".into(),
        None => media::desktop_triple(target, env::consts::ARCH)?,
    };
    let prefix = sdk::prepare(&triple)?;
    let mut env = sdk::runtime_environment(&prefix)?;
    if target == "macos" {
        env.insert("MACOSX_DEPLOYMENT_TARGET".into(), "14.0".into());
    }
    let package = if mobile {
        "hongguo-nsh-ios"
    } else {
        "hongguo-nsh"
    };
    let mut cargo = words(&["build", "--locked", "-p", package]);
    if profile == "release" {
        cargo.push("--release".into());
    }
    let targeted = requested.is_some()
        || mobile
        || profile != "release"
        || env::var_os("CARGO_BUILD_TARGET").is_some();
    if targeted {
        cargo.extend(words(&["--target", &triple]));
    }
    if target == "windows" && profile == "release" {
        cargo.extend(words(&[
            "--config",
            "crates/hongguo-nsh/windows-build.toml",
        ]));
    }
    let relink = Path::new("target/nsh-native").join(target).join("Relink");
    if mobile {
        let sdk_name = if target == "ios" {
            "iphoneos"
        } else {
            "iphonesimulator"
        };
        let sdk = output("xcrun", &words(&["--sdk", sdk_name, "--show-sdk-path"]))?;
        let linker = output("xcrun", &words(&["--sdk", sdk_name, "--find", "clang"]))?;
        env.extend(relink::environment(
            &triple,
            &prefix,
            &linker,
            &sdk,
            &relink,
            &[],
        )?);
        env.insert("SDKROOT".into(), sdk);
        env.insert("IPHONEOS_DEPLOYMENT_TARGET".into(), "16.0".into());
    }
    run("cargo", &cargo, &env, None)?;
    let mut built = absolute(env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| "target".into()))?;
    if targeted {
        built.push(&triple);
    }
    built.push(profile);
    built.push(if mobile {
        "hongguo-nsh-ios"
    } else if target == "windows" {
        "hongguo-nsh.exe"
    } else {
        "hongguo-nsh"
    });
    let stable = Path::new("target/nsh-native").join(target).join(profile);
    copy(
        &built,
        stable.join(if target == "windows" {
            "host.exe"
        } else {
            "host"
        }),
    )?;
    if target == "windows" {
        copy(prefix.join("bin/libde265.dll"), stable.join("libde265.dll"))?;
    }
    if mobile {
        relink::finish(&built, &triple, &relink)?;
        media::notices(
            &prefix,
            &triple,
            &Path::new("target/nsh-native")
                .join(target)
                .join("NativeMedia"),
        )?;
    }
    Ok(())
}

pub fn stage(args: &[String]) -> Result<()> {
    let target = arg(args, 0)?;
    let destination = absolute(arg(args, 1)?)?;
    let executable = arg(args, 2)?;
    if target == "android" {
        for folder in ["Relink", "NativeMedia"] {
            copy(
                Path::new("target/nsh-native/android").join(folder),
                destination.join(folder),
            )?;
        }
        if executable.ends_with(".apk") {
            android::verify_apk(&destination.join(executable))?;
        }
        return Ok(());
    }
    if target.starts_with("ios") {
        for folder in ["Relink", "NativeMedia"] {
            copy(
                Path::new("target/nsh-native").join(target).join(folder),
                destination
                    .parent()
                    .context("iOS package directory")?
                    .join(folder),
            )?;
        }
    } else {
        let arch = if target == "macos" {
            output(
                "lipo",
                &words(&[
                    "-archs",
                    &string(destination.join("Contents/MacOS").join(executable)),
                ]),
            )?
        } else {
            "x86_64".into()
        };
        ensure!(
            !arch.contains(' '),
            "Package each architecture separately; universal media libraries are not configured"
        );
        let triple = media::desktop_triple(target, &arch)?;
        media::copy_desktop(&sdk::prepare(&triple)?, &triple, &destination)?;
        if target == "windows" {
            return media::audit_windows(&destination);
        }
        ensure!(target == "macos", "Unsupported stage target: {target}");
        let binary = destination.join("Contents/MacOS").join(executable);
        let commands = output("otool", &words(&["-l", &string(&binary)]))?;
        for entry in
            Regex::new(r"cmd LC_RPATH[\s\S]*?\n\s*path (.+?) \(offset")?.captures_iter(&commands)
        {
            if entry[1].contains("/native-media/") {
                run(
                    "install_name_tool",
                    &words(&["-delete_rpath", &entry[1], &string(&binary)]),
                    &Environment::new(),
                    None,
                )?;
            }
        }
        media::audit_macos(&destination)?;
        let resources = destination.join("Contents/Resources");
        let icon = resources.join("AppIcon.icns");
        const ICON_HASH_LENGTH: usize = 12;
        let name = format!("AppIcon-{}", &digest(&icon)?[..ICON_HASH_LENGTH]);
        fs::rename(&icon, resources.join(format!("{name}.icns")))?;
        run(
            "/usr/libexec/PlistBuddy",
            &words(&[
                "-c",
                &format!("Set :CFBundleIconFile {name}"),
                &string(destination.join("Contents/Info.plist")),
            ]),
            &Environment::new(),
            None,
        )?;
    }
    let metadata: Value = read_json("crates/hongguo-nsh/nsh.json")?;
    let localized = destination.join(if target == "macos" {
        "Contents/Resources/zh-Hans.lproj"
    } else {
        "zh-Hans.lproj"
    });
    write(
        localized.join("InfoPlist.strings"),
        format!(
            "\"CFBundleDisplayName\" = {};\n",
            serde_json::to_string(&metadata["nativeHost"]["displayNames"]["zh-CN"])?
        ),
    )
}
