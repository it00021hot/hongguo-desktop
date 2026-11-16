use crate::{io::*, media, relink, sdk};
use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde_json::{Value, json};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

const COMPILE_SDK: &str = "37";
const MIN_API: &str = "28";
const TRIPLE: &str = "aarch64-linux-android";

fn ndk_bin(ndk: &str) -> Result<PathBuf> {
    let host = match env::consts::OS {
        "windows" => "windows-x86_64",
        "macos" => "darwin-x86_64",
        "linux" => "linux-x86_64",
        other => anyhow::bail!("Unsupported NDK host: {other}"),
    };
    Ok(Path::new(ndk)
        .join("toolchains/llvm/prebuilt")
        .join(host)
        .join("bin"))
}
fn executable(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    }
}

pub fn build(profile: &str) -> Result<()> {
    let sdk_path = required_env("ANDROID_HOME")?;
    let ndk = env::var("ANDROID_NDK_HOME")
        .or_else(|_| env::var("NDK_HOME"))
        .context("Set ANDROID_NDK_HOME or NDK_HOME")?;
    let distribution = required_env("NSH_DISTRIBUTION")?;
    let aar = Path::new(&distribution).join("embed/android/nsh.aar");
    let bin = ndk_bin(&ndk)?;
    let clang = bin.join(executable("clang"));
    let prefix = sdk::prepare(TRIPLE)?;
    let out = absolute("target/nsh-native/android")?;
    let relink = out.join("Relink");
    let flags = format!("--target=aarch64-linux-android{MIN_API}");
    let mut env = sdk::runtime_environment(&prefix)?;
    env.extend(Environment::from([
        ("ANDROID_NDK_ROOT".into(), ndk.clone()),
        ("NSH_ANDROID_MIN_SDK".into(), MIN_API.into()),
        (
            "CMAKE_TOOLCHAIN_FILE_aarch64_linux_android".into(),
            string(absolute("scripts/nsh-android.cmake")?),
        ),
        ("CC_aarch64_linux_android".into(), string(&clang)),
        (
            "CXX_aarch64_linux_android".into(),
            string(bin.join(executable("clang++"))),
        ),
        (
            "AR_aarch64_linux_android".into(),
            string(bin.join(executable("llvm-ar"))),
        ),
        ("CFLAGS_aarch64_linux_android".into(), flags.clone()),
        ("CXXFLAGS_aarch64_linux_android".into(), flags.clone()),
        (
            "BINDGEN_EXTRA_CLANG_ARGS_aarch64_linux_android".into(),
            format!("--sysroot=\"{}\" {flags}", bin.join("../sysroot").display()),
        ),
    ]));
    env.extend(relink::environment(
        TRIPLE,
        &prefix,
        &string(&clang),
        &ndk,
        &relink,
        &[flags, "-Wl,--no-undefined".into()],
    )?);
    let mut args = words(&[
        "build",
        "--locked",
        "-p",
        "hongguo-nsh-android",
        "--target",
        TRIPLE,
    ]);
    if profile == "release" {
        args.push("--release".into());
    }
    run("cargo", &args, &env, None)?;
    let target = absolute(env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| "target".into()))?;
    let binary = target
        .join(TRIPLE)
        .join(profile)
        .join("libhongguo_nsh_android.so");
    relink::finish(&binary, TRIPLE, &relink)?;
    audit_elf(&binary, &bin.join(executable("llvm-readelf")))?;
    let runtime = out.join("runtime");
    remove(&runtime)?;
    copy(&binary, runtime.join("lib/arm64-v8a/libnsh.so"))?;
    let platform = build_runtime(&aar, Path::new(&sdk_path), &out)?;
    let java = Path::new("crates/hongguo-nsh-android/android/app/src/main");
    let classes = out.join("host-classes");
    remove(&classes)?;
    fs::create_dir_all(&classes)?;
    let jar = out.join("runtime-java/classes.jar");
    let separator = if cfg!(windows) { ";" } else { ":" };
    javac(
        &format!("{}{separator}{}", platform.display(), jar.display()),
        &classes,
        &java.join("java"),
        &out.join("host-javac.args"),
    )?;
    let host_jar = out.join("host.jar");
    run(
        "jar",
        &words(&["cf", &string(&host_jar), "-C", &string(&classes), "."]),
        &Environment::new(),
        None,
    )?;
    let tools = build_tools(Path::new(&sdk_path))?;
    run(
        "java",
        &words(&[
            "-cp",
            &string(tools.join("lib/d8.jar")),
            "com.android.tools.r8.D8",
            "--lib",
            &string(&platform),
            "--min-api",
            MIN_API,
            "--output",
            &string(&runtime),
            &string(&jar),
            &string(&host_jar),
        ]),
        &Environment::new(),
        None,
    )?;
    for folder in ["res", "assets"] {
        copy(java.join(folder), runtime.join(folder))?;
    }
    for name in ["LICENSE", "NOTICE", "THIRD-PARTY-NOTICES.md"] {
        copy(name, runtime.join("assets/licenses").join(name))?;
    }
    let metadata: Value = read_json("crates/hongguo-nsh/nsh.json")?;
    for (folder, name) in [
        ("values", &metadata["name"]),
        (
            "values-zh",
            &metadata["nativeHost"]["displayNames"]["zh-CN"],
        ),
    ] {
        write(
            runtime.join("res").join(folder).join("strings.xml"),
            format!(
                "<resources><string name=\"app_name\">{}</string></resources>",
                xml(name.as_str().context("Application name")?)
            ),
        )?;
    }
    copy(
        "assets/icons/512x512.png",
        runtime.join("res/drawable/guava_icon.png"),
    )?;
    let manifest = text(java.join("AndroidManifest.xml"))?
        .replacen(
            "<manifest ",
            &format!(
                "<manifest package=\"{}\" ",
                metadata["id"].as_str().context("App ID")?
            ),
            1,
        )
        .replacen(
            "<application ",
            "<uses-sdk android:minSdkVersion=\"28\" /><application ",
            1,
        )
        .replacen(
            "android:name=\".MainActivity\"",
            "android:name=\"com.hongguo.nsh.MainActivity\"",
            1,
        );
    write(runtime.join("AndroidManifest.xml"), manifest)?;
    media::notices(&prefix, TRIPLE, &out.join("NativeMedia"))?;
    relink::android_source(&relink, &out)
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn version(path: &Path) -> Vec<u32> {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .split('.')
        .map(|s| s.parse().unwrap_or(0))
        .collect()
}

fn build_tools(sdk: &Path) -> Result<PathBuf> {
    let pattern = Regex::new(r"^\d+\.\d+\.\d+$")?;
    let mut candidates: Vec<_> = list(sdk.join("build-tools"))?
        .into_iter()
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| pattern.is_match(&n.to_string_lossy()))
        })
        .collect();
    candidates.sort_by_key(|p| version(p));
    candidates
        .pop()
        .context("Stable Android SDK build-tools are required")
}

fn android_platform(sdk: &Path) -> Result<PathBuf> {
    let mut candidates = Vec::new();
    for folder in list(sdk.join("platforms"))? {
        let properties = folder.join("source.properties");
        if !properties.exists() {
            continue;
        }
        let text = text(properties)?;
        let values: std::collections::BTreeMap<_, _> = text
            .lines()
            .filter_map(|l| l.trim().split_once('='))
            .collect();
        if !matches!(values.get("AndroidVersion.ApiLevel"), Some(&"37" | &"37.0"))
            || values
                .get("AndroidVersion.CodeName")
                .is_some_and(|s| !s.is_empty())
        {
            continue;
        }
        let revision = values
            .get("Pkg.Revision")
            .context("Android SDK revision missing")?;
        let version: Vec<u32> = revision
            .split('.')
            .map(str::parse)
            .collect::<Result<_, _>>()?;
        candidates.push((version, folder.join("android.jar")));
    }
    candidates.sort();
    let platform = candidates.pop().context("Android SDK 37 is required")?.1;
    ensure!(platform.is_file(), "Android SDK 37 android.jar missing");
    Ok(platform)
}

fn java_sources(directory: &Path) -> Result<Vec<PathBuf>> {
    Ok(walk(directory)?
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "java"))
        .collect())
}

fn javac(classpath: &str, classes: &Path, source: &Path, response: &Path) -> Result<()> {
    let sources = java_sources(source)?;
    ensure!(
        !sources.is_empty(),
        "Java source directory is empty: {}",
        source.display()
    );
    let mut args = words(&[
        "--release",
        "17",
        "-encoding",
        "UTF-8",
        "-classpath",
        classpath,
        "-d",
        &string(classes),
    ]);
    args.extend(sources.iter().map(string));
    let contents = args
        .iter()
        .map(|s| serde_json::to_string(&s.replace('\\', "/")))
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    write(response, contents)?;
    run(
        "javac",
        &[format!("@{}", response.display())],
        &Environment::new(),
        None,
    )
}

fn build_runtime(aar: &Path, sdk: &Path, out: &Path) -> Result<PathBuf> {
    let platform = android_platform(sdk)?;
    let source = absolute("vendor/nsh/crates/nsh-platform-android/java")?;
    let work = out.join("runtime-java");
    remove(&work)?;
    let classes = work.join("compiled");
    fs::create_dir_all(&classes)?;
    let entries = ["AndroidManifest.xml", "R.txt", "proguard.txt"];
    let mut args = words(&["xf", &string(aar)]);
    args.extend(words(&entries));
    run("jar", &args, &Environment::new(), Some(&work))?;
    javac(
        &string(&platform),
        &classes,
        &source,
        &work.join("javac.args"),
    )?;
    let jar = work.join("classes.jar");
    run(
        "jar",
        &words(&["cf", &string(&jar), "-C", &string(&classes), "."]),
        &Environment::new(),
        None,
    )?;
    let runtime = out.join("nsh-runtime.aar");
    let mut args = words(&["cf", &string(&runtime), "classes.jar"]);
    args.extend(words(&entries));
    run("jar", &args, &Environment::new(), Some(&work))?;
    let sources: Vec<_> = java_sources(&source)?
        .into_iter()
        .map(|p| Ok(json!({"path": slash(p.strip_prefix(&source)?), "sha256": digest(&p)?})))
        .collect::<Result<_>>()?;
    write_json(
        out.join("runtime-source.json"),
        &json!({"schemaVersion":1,"compileSdk":COMPILE_SDK.parse::<u32>()?,"androidJarPath":platform,"embeddingSdkSha256":digest(aar)?,"androidJarSha256":digest(&platform)?,"runtimeSha256":digest(&runtime)?,"classesSha256":digest(jar)?,"sources":sources}),
    )?;
    Ok(platform)
}

pub fn audit_elf(binary: &Path, readelf: &Path) -> Result<()> {
    let text = output(
        readelf,
        &words(&[
            "--file-header",
            "--dynamic",
            "--dyn-syms",
            "--program-headers",
            "--wide",
            &string(binary),
        ]),
    )?;
    ensure!(
        Regex::new(r"Class:\s+ELF64")?.is_match(&text)
            && Regex::new(r"Machine:\s+AArch64")?.is_match(&text),
        "Android output must be AArch64 ELF64"
    );
    const SYSTEM: &[&str] = &[
        "libaaudio.so",
        "libm.so",
        "liblog.so",
        "libandroid.so",
        "libmediandk.so",
        "libdl.so",
        "libEGL.so",
        "libGLESv2.so",
        "libGLESv3.so",
        "libvulkan.so",
        "libnativewindow.so",
        "libsync.so",
        "libc.so",
    ];
    let needed = Regex::new(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]")?;
    ensure!(
        needed.is_match(&text),
        "Android output has no inspectable runtime dependencies"
    );
    for entry in needed.captures_iter(&text) {
        ensure!(
            SYSTEM.contains(&&entry[1]),
            "Unapproved Android runtime: {}",
            &entry[1]
        );
    }
    ensure!(
        !Regex::new(r"\bUND\s+(_Z\S+)")?.is_match(&text),
        "Android static C++ runtime has unresolved symbols"
    );
    let align = Regex::new(r"(?im)^\s+LOAD\s+.+?\s+(0x[\da-f]+)\s*$")?;
    ensure!(align.is_match(&text), "Android LOAD segments missing");
    const MIN_ALIGNMENT: u64 = 16_384;
    for entry in align.captures_iter(&text) {
        ensure!(
            u64::from_str_radix(&entry[1][2..], 16)? >= MIN_ALIGNMENT,
            "Android LOAD must support 16 KiB pages"
        );
    }
    Ok(())
}

pub fn verify_apk(apk: &Path) -> Result<()> {
    let tools = build_tools(Path::new(&required_env("ANDROID_HOME")?))?;
    run(
        tools.join(executable("zipalign")),
        &words(&["-c", "-P", "16", "4", &string(apk)]),
        &Environment::new(),
        None,
    )?;
    if cfg!(windows) {
        run(
            "cmd.exe",
            &words(&[
                "/d",
                "/c",
                &string(tools.join("apksigner.bat")),
                "verify",
                &string(apk),
            ]),
            &Environment::new(),
            None,
        )?;
    } else {
        run(
            tools.join("apksigner"),
            &words(&["verify", &string(apk)]),
            &Environment::new(),
            None,
        )?;
    }
    let temporary = tempfile::Builder::new()
        .prefix("guava-apk-native-")
        .tempdir()?;
    run(
        "jar",
        &words(&["xf", &string(apk), "lib/arm64-v8a/libnsh.so"]),
        &Environment::new(),
        Some(temporary.path()),
    )?;
    ensure!(
        digest("target/nsh-native/android/runtime/lib/arm64-v8a/libnsh.so")?
            == digest(temporary.path().join("lib/arm64-v8a/libnsh.so"))?,
        "APK native library differs from captured relinking materials"
    );
    Ok(())
}
