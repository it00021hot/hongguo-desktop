use crate::{io::*, pe, sdk};
use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde_json::Value;
use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

const SYSTEM_DLLS: &str = "advapi32 avrt bcrypt bcryptprimitives cfgmgr32 combase comctl32 comdlg32 coremessaging crypt32 cryptbase d2d1 d3d11 d3d12 d3dcompiler_47 dcomp dnsapi dwrite dwmapi dxgi gdi32 imm32 iphlpapi kernel32 kernelbase msimg32 msvfw32 ncrypt netapi32 ntdll ole32 oleacc oleaut32 opengl32 powrprof propsys psapi rpcrt4 secur32 setupapi shell32 shcore shlwapi uiautomationcore user32 userenv usp10 uxtheme version winhttp wininet winmm winspool ws2_32 wtsapi32 wintrust windowscodecs normaliz hid dbghelp mf mfplat mfreadwrite mfcore mfplay evr msmpeg2adec msmpeg2vdec xaudio2_8 xaudio2_9";

pub fn desktop_triple(platform: &str, arch: &str) -> Result<String> {
    match (platform, arch) {
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc".into()),
        ("macos", "aarch64" | "arm64") => Ok("aarch64-apple-darwin".into()),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin".into()),
        _ => anyhow::bail!("Unsupported desktop media target: {platform}/{arch}"),
    }
}

pub fn notices(prefix: &Path, triple: &str, destination: &Path) -> Result<()> {
    let root = prefix.parent().context("SDK root")?;
    let sdk = sdk::verify(triple, root)?;
    remove(destination)?;
    fs::create_dir_all(destination)?;
    for name in ["LICENSE", "NOTICE", "THIRD-PARTY-NOTICES.md"] {
        copy(name, destination.join(name))?;
    }
    for directory in ["licenses", "sources", "build-info"] {
        copy(
            prefix.join(directory),
            destination.join("libde265").join(directory),
        )?;
    }
    copy(
        prefix.join("manifest.json"),
        destination.join("libde265/source-manifest.json"),
    )?;
    copy(root.join("sdk.json"), destination.join("sdk.json"))?;
    let vvdec = Path::new("crates/hongguo-vvdec-sys");
    let notices = destination.join("vvdec");
    for name in ["LICENSE.txt", "AUTHORS.md"] {
        copy(vvdec.join("vendor/vvdec").join(name), notices.join(name))?;
    }
    copy(vvdec.join("SOURCE.md"), notices.join("SOURCE.md"))?;
    copy(
        sdk.vvdec.join("manifest.json"),
        notices.join("source-manifest.json"),
    )?;
    copy(vvdec.join("vendor/vvdec"), notices.join("source"))?;
    for directory in ["patches", "build-info"] {
        let path = sdk.vvdec.join(directory);
        if path.exists() {
            copy(path, notices.join(directory))?;
        }
    }
    copy(
        vvdec.join("vendor/vvdec/thirdparty/simde/COPYING"),
        notices.join("SIMDe-COPYING"),
    )?;
    copy(
        "vendor/nsh/LICENSE-APACHE",
        destination.join("NSH-LICENSE-APACHE"),
    )?;
    copy(
        "crates/hongguo-nsh/src/ui/icons/LICENSE",
        destination.join("Lucide-LICENSE"),
    )?;
    for name in ["LICENSE.md", "graph.json"] {
        copy(
            Path::new("crates/hongguo-video-gpu/shaders/enhancement/anime4k").join(name),
            destination.join("anime4k").join(name),
        )?;
    }
    if triple.contains("android") {
        let ndk = env::var("ANDROID_NDK_HOME")
            .or_else(|_| env::var("NDK_HOME"))
            .or_else(|_| env::var("ANDROID_NDK_ROOT"))?;
        for name in ["NOTICE", "NOTICE.toolchain", "source.properties"] {
            copy(
                Path::new(&ndk).join(name),
                destination.join("android-runtime").join(name),
            )?;
        }
        copy(
            "vendor/nsh/crates/nsh-platform-android/java",
            destination.join("nsh-android-java"),
        )?;
    }
    crate::relink::distribute_tool(&destination.join("BuildTool"))?;
    for entry in list("scripts")? {
        let name = entry.file_name().context("Script name")?.to_string_lossy();
        if name.starts_with("native-") || name == "nsh-android.cmake" {
            copy(
                &entry,
                destination.join("build-scripts").join(name.as_ref()),
            )?;
        }
    }
    copy("docs/native-media-build.md", destination.join("BUILD.md"))?;
    write(
        destination.join("README.txt"),
        "libde265 1.1.3: LGPL-3.0-or-later. VVdeC: separate Clear BSD license.\nCorresponding source, licenses, configuration and artifact hashes: libde265/ and vvdec/.\nThe source manifest describes the build prefix before bundle relocation/signing.\nDesktop: replace the ABI-compatible libde265 library in the application runtime directory.\nmacOS: libraries are in Contents/Frameworks. Re-sign each replaced dylib, then the app:\n  codesign --force --sign - --timestamp=none <dylib>\n  codesign --force --sign - --timestamp=none <app>\nA modified app needs an identity/profile authorized by its installer on Apple devices.\nMobile static packages require companion relinking material; see Relink/README.md.\n",
    )
}

pub fn copy_desktop(prefix: &Path, triple: &str, destination: &Path) -> Result<()> {
    sdk::verify(triple, prefix.parent().context("SDK root")?)?;
    let manifest: Value = read_json(prefix.join("manifest.json"))?;
    let windows = triple.contains("windows");
    let runtime = if windows {
        destination.into()
    } else {
        destination.join("Contents/Frameworks")
    };
    fs::create_dir_all(&runtime)?;
    let pattern = Regex::new(if windows {
        r"^bin/[^/]+\.dll$"
    } else {
        r"^lib/[^/]+\.dylib$"
    })?;
    let artifacts: Vec<sdk::Artifact> = serde_json::from_value(manifest["artifacts"].clone())?;
    for artifact in artifacts {
        if pattern.is_match(&artifact.path) {
            let source = prefix.join(relative(&artifact.path)?);
            copy(
                &source,
                runtime.join(source.file_name().context("Library name")?),
            )?;
        }
    }
    notices(
        prefix,
        triple,
        &destination.join(if windows {
            "NativeMedia"
        } else {
            "Contents/Resources/NativeMedia"
        }),
    )
}

pub fn audit_windows(destination: &Path) -> Result<()> {
    let images: Vec<PathBuf> = list(destination)?
        .into_iter()
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("dll"))
        })
        .collect();
    let dlls: Vec<_> = images
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll")))
        .collect();
    ensure!(
        images.len() - dlls.len() == 1,
        "Windows bundle must contain one EXE"
    );
    ensure!(
        dlls.len() == 1 && dlls[0].file_name().is_some_and(|n| n == "libde265.dll"),
        "Windows bundle must contain exactly the pinned libde265 DLL"
    );
    let available: HashSet<String> = images
        .iter()
        .map(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase()
        })
        .collect();
    let system: HashSet<&str> = SYSTEM_DLLS.split_whitespace().collect();
    let api = Regex::new(
        r"^api-ms-win-(?:core|security|eventing|service|shell|shcore|ntuser|power)-[\w-]+\.dll$",
    )?;
    let metadata: Value = read_json("crates/hongguo-nsh/nsh.json")?;
    for path in images {
        let bytes = fs::read(&path)?;
        let imports = if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
        {
            pe::imports(&bytes, true)?
        } else {
            pe::validate(&bytes, metadata["id"].as_str().context("App ID")?)?
        };
        for dependency in imports {
            let normalized = dependency.to_lowercase();
            ensure!(
                available.contains(&normalized)
                    || system.contains(normalized.trim_end_matches(".dll"))
                    || api.is_match(&normalized),
                "Unbundled or unapproved Windows dependency: {} -> {dependency}",
                path.display()
            );
        }
    }
    Ok(())
}

pub fn audit_macos(destination: &Path) -> Result<()> {
    let frameworks = destination.join("Contents/Frameworks");
    let mut libraries = list(&frameworks)?
        .iter()
        .map(fs::canonicalize)
        .collect::<std::io::Result<Vec<_>>>()?;
    libraries.sort();
    libraries.dedup();
    ensure!(
        libraries.len() == 1
            && libraries[0]
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("libde265.")),
        "macOS bundle must contain exactly the libde265 runtime"
    );
    libraries.push(destination.join("Contents/MacOS/guava"));
    for path in libraries {
        for line in output("otool", &words(&["-L", &string(&path)]))?
            .lines()
            .skip(1)
        {
            let dependency = line.trim().split(" (").next().unwrap_or("");
            if dependency.is_empty()
                || dependency.starts_with("/System/Library/")
                || dependency.starts_with("/usr/lib/")
            {
                continue;
            }
            let name = dependency
                .strip_prefix("@rpath/")
                .context("macOS dependency is not bundle-relative")?;
            ensure!(
                !name.contains('/') && frameworks.join(name).exists(),
                "macOS runtime dependency is not bundled: {dependency}"
            );
        }
    }
    Ok(())
}

pub fn verify_workspace() -> Result<()> {
    let metadata: Value = serde_json::from_str(&output(
        "cargo",
        &words(&["metadata", "--locked", "--format-version=1"]),
    )?)?;
    let root = Path::new("vendor/nsh").canonicalize()?;
    let packages: Vec<_> = metadata["packages"]
        .as_array()
        .context("Cargo packages")?
        .iter()
        .filter(|p| p["name"].as_str().is_some_and(|s| s.starts_with("nsh-")))
        .collect();
    ensure!(
        !packages.is_empty(),
        "Cargo metadata contains no NSH packages"
    );
    for package in packages {
        ensure!(
            package["source"].is_null()
                && Path::new(
                    package["manifest_path"]
                        .as_str()
                        .context("Cargo manifest")?
                )
                .canonicalize()?
                .starts_with(&root),
            "NSH must resolve to vendored source: {}",
            package["name"]
        );
    }
    let graph = output(
        "cargo",
        &words(&[
            "tree",
            "--locked",
            "-p",
            "hongguo-nsh",
            "--edges",
            "normal",
            "--prefix",
            "none",
        ]),
    )?;
    ensure!(
        !Regex::new(r"(?m)^(?:nsh-js|quickjs[^ ]*) v")?.is_match(&graph),
        "Pure Rust host must not include QuickJS"
    );
    Ok(())
}
