use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;

pub const TRIPLES: &[&str] = &[
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "aarch64-linux-android",
    "aarch64-apple-ios",
    "aarch64-apple-ios-sim",
];
pub const HEVC_SHA: &str = "7ffe3220bba8282cc6d4a2bcf36ac1b6d2899f402c335f7df377317414a025b1";
pub const HEVC_COMMIT: &str = "ba62bf4cfb3242f3bf0a45617ff09e35236e4d82";
pub const VVDEC_SHA: &str = "5c334557a33cd93e981b84ba0e77126ef970a38e2481b67f0475db40f75379b8";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub triple: String,
    pub arch: String,
    pub os: String,
    pub minimum_os: Value,
    pub crt: String,
    pub cxx_runtime: String,
    pub cpu_baseline: String,
    pub cpp_abi: String,
}

pub fn target(triple: &str) -> Result<Target> {
    ensure!(
        TRIPLES.contains(&triple),
        "Unsupported native codec target: {triple}"
    );
    let os = if triple.contains("windows") {
        "windows"
    } else if triple.contains("android") {
        "android"
    } else if triple.contains("darwin") {
        "macos"
    } else {
        "ios"
    };
    let arch = if triple.starts_with("aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    let minimum_os = match os {
        "windows" => "10.0.17763".into(),
        "macos" => "14.0".into(),
        "android" => 28.into(),
        _ => "16.0".into(),
    };
    Ok(Target {
        triple: triple.into(),
        arch: arch.into(),
        os: os.into(),
        minimum_os,
        crt: if os == "windows" { "static" } else { "system" }.into(),
        cxx_runtime: match os {
            "windows" => "msvc-static",
            "android" => "libc++_static",
            _ => "libc++",
        }
        .into(),
        cpu_baseline: if arch == "x86_64" {
            "x86-64"
        } else {
            "armv8-a"
        }
        .into(),
        cpp_abi: if os == "windows" { "msvc" } else { "itanium" }.into(),
    })
}

pub fn vvdec_target(triple: &str) -> Result<Target> {
    let mut target = target(triple)?;
    if target.os == "macos" {
        target.minimum_os = if target.arch == "x86_64" {
            "10.15"
        } else {
            "11.0"
        }
        .into();
    }
    Ok(target)
}

pub fn matches(pattern: &str, text: &str) -> Result<bool> {
    Ok(Regex::new(pattern)?.is_match(text))
}
pub fn captures(pattern: &str, text: &str) -> Result<Vec<String>> {
    Ok(Regex::new(pattern)?
        .captures_iter(text)
        .map(|c| c[1].trim().into())
        .collect())
}
pub fn capture(pattern: &str, text: &str) -> Result<String> {
    captures(pattern, text)?
        .into_iter()
        .next()
        .with_context(|| format!("Missing build evidence: {pattern}"))
}

fn supported_version(value: &str, maximum: &Value) -> Result<bool> {
    if !matches(r"^\d+\.\d+(?:\.\d+)?$", value)? {
        return Ok(false);
    }
    let mut actual: Vec<u32> = value.split('.').map(str::parse).collect::<Result<_, _>>()?;
    let mut limit: Vec<u32> = maximum
        .as_str()
        .context("OS version string")?
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    actual.resize(3, 0);
    limit.resize(3, 0);
    Ok(actual <= limit)
}

fn strings(value: &Value) -> Result<Vec<&str>> {
    let values = value.as_array().context("Missing object inspection list")?;
    ensure!(!values.is_empty(), "Empty object inspection list");
    values
        .iter()
        .map(|v| v.as_str().context("Object inspection string"))
        .collect()
}

pub fn validate_vvdec(target: &Target, evidence: &Value) -> Result<()> {
    ensure!(
        evidence["configuration"] == "Release" && evidence["sourceMatches"] == true,
        "VVdeC requires a Release build of current vendored source"
    );
    let arch = if target.arch == "aarch64" && target.os != "android" {
        "arm64"
    } else {
        &target.arch
    };
    ensure!(
        evidence["architecture"] == arch,
        "VVdeC archive architecture mismatch"
    );
    if target.os == "windows" {
        ensure!(
            strings(&evidence["machines"])?
                .iter()
                .all(|s| *s == "IMAGE_FILE_MACHINE_AMD64"),
            "VVdeC contains non-AMD64 objects"
        );
        let directives = evidence["directives"]
            .as_str()
            .context("VVdeC directives")?;
        ensure!(
            matches("RuntimeLibrary=MT_StaticRelease", directives)?
                && matches("(?i)DEFAULTLIB:libcmt", directives)?
                && !matches(
                    "(?i)DEFAULTLIB:(?:msvcrt|msvcprt)|RuntimeLibrary=MD_",
                    directives
                )?,
            "VVdeC must use static release MSVC CRT"
        );
        let flags = evidence["genericFlags"].as_str().unwrap_or("");
        ensure!(
            !matches(r"/arch:AVX|\s-mavx", flags)?,
            "VVdeC exceeds CPU baseline"
        );
        for march in captures(r"\s-march=(\S+)", flags)? {
            ensure!(march == "x86-64", "VVdeC exceeds CPU baseline");
        }
    } else if target.os == "android" {
        ensure!(
            strings(&evidence["machines"])?
                .iter()
                .all(|s| *s == "AArch64"),
            "VVdeC contains non-AArch64 objects"
        );
        ensure!(
            evidence["compilerTarget"]
                == format!("aarch64-none-linux-android{}", target.minimum_os),
            "VVdeC compiler API mismatch"
        );
    } else {
        let platform = if target.os == "macos" {
            "1"
        } else if target.triple.ends_with("-sim") {
            "7"
        } else {
            "2"
        };
        ensure!(
            strings(&evidence["platforms"])?
                .iter()
                .all(|s| *s == platform),
            "VVdeC Mach-O platform mismatch"
        );
        for version in strings(&evidence["minimumVersions"])? {
            ensure!(
                supported_version(version, &target.minimum_os)?,
                "VVdeC requires a newer OS than declared"
            );
        }
    }
    Ok(())
}

pub fn validate_hevc(target: &Target, inspection: &str, commands: &Value) -> Result<()> {
    let commands = commands.as_array().context("Compile command list")?;
    if target.os == "windows" {
        ensure!(
            inspection.contains("IMAGE_FILE_MACHINE_AMD64"),
            "HEVC library is not AMD64"
        );
        let generic = commands
            .iter()
            .find(|e| {
                e["file"]
                    .as_str()
                    .is_some_and(|s| s.ends_with("/de265.cc") || s.ends_with("\\de265.cc"))
            })
            .and_then(|e| e["command"].as_str())
            .context("Missing HEVC generic compilation unit")?;
        ensure!(
            matches(r"(?:^|\s)[/-]MT(?:\s|$)", generic)?
                && !matches(r"/arch:AVX|\s-mavx", generic)?,
            "HEVC must use static release CRT and baseline CPU"
        );
        return Ok(());
    }
    let formats = captures(r"(?m)^Format: (.+)$", inspection)?;
    ensure!(!formats.is_empty(), "Missing HEVC object format");
    for format in formats {
        ensure!(
            if target.os == "android" {
                format == "elf64-littleaarch64"
            } else {
                format.contains(if target.arch == "aarch64" {
                    "arm64"
                } else {
                    "x86-64"
                })
            },
            "HEVC target architecture mismatch"
        );
    }
    if target.os == "android" {
        ensure!(
            !commands.is_empty()
                && commands
                    .iter()
                    .all(
                        |c| c["command"].as_str().is_some_and(|s| s.contains(&format!(
                            "--target=aarch64-none-linux-android{}",
                            target.minimum_os
                        )))
                    ),
            "HEVC Android compiler API mismatch"
        );
        return Ok(());
    }
    let expected = if target.os == "macos" {
        "macos"
    } else if target.triple.ends_with("-sim") {
        "iossimulator"
    } else {
        "ios"
    };
    let platforms = captures(r"(?m)^\s+Platform: (.+)$", inspection)?;
    let versions = captures(r"(?m)^\s+Version: (.+)$", inspection)?;
    ensure!(
        !platforms.is_empty() && platforms.iter().all(|s| s == expected) && !versions.is_empty(),
        "HEVC Mach-O platform missing or mismatched"
    );
    for version in versions {
        ensure!(
            supported_version(&version, &target.minimum_os)?,
            "HEVC minimum OS mismatch"
        );
    }
    Ok(())
}
