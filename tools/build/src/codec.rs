use crate::{evidence::*, io::*};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub fn hevc_source() -> serde_json::Value {
    json!({"version":"1.1.3","commit":HEVC_COMMIT,"url":format!("https://codeload.github.com/strukturag/libde265/tar.gz/{HEVC_COMMIT}"),"sha256":HEVC_SHA})
}

pub fn cross_tools() -> Result<()> {
    let directory = Path::new("target/nsh-cross-tools");
    fs::create_dir_all(directory)?;
    for name in ["nsh-windows-clang", "nsh-windows-rc", "rc"] {
        copy(
            env::current_exe()?,
            directory.join(format!("{name}{}", env::consts::EXE_SUFFIX)),
        )?;
    }
    Ok(())
}

pub fn clang_launcher(args: &[String]) -> Result<()> {
    let compiler = required_env("NSH_WINDOWS_CLANG")?;
    let mapped = args
        .iter()
        .skip(1)
        .map(|arg| {
            let driver = [
                "-Werror",
                "-Wno-unused-command-line-argument",
                "-W0",
                "-W1",
                "-W2",
                "-W3",
                "-W4",
            ]
            .contains(&arg.as_str());
            if arg == "-pedantic" || (arg.starts_with("-W") && !driver) {
                format!("/clang:{arg}")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>();
    run(compiler, &mapped, &Environment::new(), None)
}

pub fn resource_compiler(args: &[String]) -> Result<()> {
    let compiler = required_env("NSH_WINDOWS_RC")?;
    let mut mapped = Vec::new();
    let mut args = args.iter();
    while let Some(argument) = args.next() {
        let option = argument.to_lowercase();
        if option == "/fo" {
            mapped.extend(words(&[
                "-fo",
                args.next()
                    .context("Missing resource output path after /fo")?,
            ]));
        } else if option.starts_with("/fo") {
            mapped.extend(words(&["-fo", &argument[3..]]));
        } else if option == "/nologo" {
            mapped.push("-nologo".into());
        } else {
            mapped.extend(words(&["--", argument]));
        }
    }
    run(compiler, &mapped, &Environment::new(), None)
}

pub fn build(args: &[String]) -> Result<()> {
    let selection = arg(args, 0)?;
    let selected: Vec<_> = if selection == "all" {
        TRIPLES.to_vec()
    } else {
        vec![selection]
    };
    for triple in selected {
        println!("Built codec: {triple} -> {}", build_hevc(triple)?.display());
    }
    Ok(())
}

fn build_hevc(triple: &str) -> Result<PathBuf> {
    let target = target(triple)?;
    let parent = absolute("target/native-codec-build")?;
    fs::create_dir_all(&parent)?;
    let archive = parent.join(format!("libde265-{HEVC_COMMIT}.tar.gz"));
    if !archive.exists() {
        let work = tempfile::Builder::new()
            .prefix("source-download-")
            .tempdir_in(&parent)?;
        let path = work.path().join("source.tar.gz");
        download(hevc_source()["url"].as_str().context("Source URL")?, &path)?;
        crate::sdk::verify_bytes(&path, HEVC_SHA, None)?;
        fs::rename(path, &archive)?;
    }
    crate::sdk::verify_bytes(&archive, HEVC_SHA, None)?;
    let work = tempfile::Builder::new()
        .prefix(&format!("{triple}-"))
        .tempdir_in(&parent)?
        .keep();
    run(
        system_tool("tar")?,
        &words(&["-xzf", &string(&archive), "-C", &string(&work)]),
        &Environment::new(),
        None,
    )?;
    let source = work.join(format!("libde265-{HEVC_COMMIT}"));
    let build = work.join("build");
    let install = work.join("install");
    let mut env = Environment::new();
    let mut options = words(&[
        "-DCMAKE_BUILD_TYPE=Release",
        "-DCMAKE_EXPORT_COMPILE_COMMANDS=ON",
        if ["windows", "macos"].contains(&target.os.as_str()) {
            "-DBUILD_SHARED_LIBS=ON"
        } else {
            "-DBUILD_SHARED_LIBS=OFF"
        },
        "-DENABLE_DECODER=OFF",
        "-DENABLE_ENCODER=OFF",
        "-DENABLE_SDL=OFF",
        "-DENABLE_SHERLOCK265=OFF",
        "-DENABLE_INTERNAL_DEVELOPMENT_TOOLS=OFF",
        "-DWITH_FUZZERS=OFF",
    ]);
    options.push(format!("-DCMAKE_INSTALL_PREFIX={}", install.display()));
    if target.os == "windows" {
        options.push(format!("-DHONGGUO_LIBDE265_SOURCE={}", source.display()));
    }
    options.extend(platform_options(&target, &mut env)?);
    let entry = if target.os == "windows" {
        absolute("scripts/native-codec-cmake")?
    } else {
        source.clone()
    };
    let mut args = words(&["-S", &string(entry), "-B", &string(&build), "-G", "Ninja"]);
    args.extend(options.clone());
    run("cmake", &args, &env, None)?;
    run(
        "cmake",
        &words(&["--build", &string(&build), "--parallel", "4"]),
        &env,
        None,
    )?;
    run("cmake", &words(&["--install", &string(&build)]), &env, None)?;
    let compiler_file = list(build.join("CMakeFiles"))?
        .into_iter()
        .map(|p| p.join("CMakeCXXCompiler.cmake"))
        .find(|p| p.is_file())
        .context("CMake compiler identity missing")?;
    let identity = text(compiler_file)?;
    let mut compiler = Vec::new();
    for key in [
        "CMAKE_CXX_COMPILER",
        "CMAKE_CXX_COMPILER_ID",
        "CMAKE_CXX_COMPILER_VERSION",
    ] {
        compiler.push(format!(
            "{key}={}",
            capture(&format!(r#"set\({key} "([^"]+)"\)"#), &identity)?
        ));
    }
    write_json(
        install.join("build-evidence.json"),
        &json!({"source":hevc_source(),"target":target,"options":options,"compiler":compiler.join("\n"),"sourceArchive":archive,"sourceDirectory":source,"buildDirectory":build}),
    )?;
    write_json(
        parent.join(format!("{triple}.json")),
        &json!({"install":install,"work":work,"target":target}),
    )?;
    Ok(install)
}

fn platform_options(target: &Target, env: &mut Environment) -> Result<Vec<String>> {
    if target.os == "windows" {
        return windows_options(env);
    }
    if target.os == "android" {
        let ndk = env::var("NDK_HOME")
            .or_else(|_| env::var("ANDROID_NDK_ROOT"))
            .context("NDK_HOME is required for Android source rebuilds")?;
        return Ok(vec![
            format!("-DCMAKE_TOOLCHAIN_FILE={ndk}/build/cmake/android.toolchain.cmake"),
            "-DANDROID_ABI=arm64-v8a".into(),
            format!("-DANDROID_PLATFORM=android-{}", target.minimum_os),
            "-DANDROID_STL=c++_static".into(),
        ]);
    }
    ensure!(
        cfg!(target_os = "macos"),
        "Apple source rebuild requires Xcode"
    );
    let sdk = if target.os == "macos" {
        "macosx"
    } else if target.triple.ends_with("-sim") {
        "iphonesimulator"
    } else {
        "iphoneos"
    };
    let mut args = vec![
        format!(
            "-DCMAKE_OSX_SYSROOT={}",
            output("xcrun", &words(&["--sdk", sdk, "--show-sdk-path"]))?
        ),
        format!(
            "-DCMAKE_OSX_ARCHITECTURES={}",
            if target.arch == "aarch64" {
                "arm64"
            } else {
                &target.arch
            }
        ),
        format!(
            "-DCMAKE_OSX_DEPLOYMENT_TARGET={}",
            target.minimum_os.as_str().context("Apple minimum OS")?
        ),
        "-DCMAKE_INSTALL_NAME_DIR=@rpath".into(),
    ];
    if target.os != "macos" {
        args.extend(words(&[
            "-DCMAKE_SYSTEM_NAME=iOS",
            "-DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY",
        ]));
    }
    Ok(args)
}

fn windows_options(env: &mut Environment) -> Result<Vec<String>> {
    let mut options = words(&[
        "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded",
        "-DCMAKE_POLICY_DEFAULT_CMP0091=NEW",
    ]);
    if cfg!(windows) {
        return Ok(options);
    }
    ensure!(
        cfg!(target_os = "macos"),
        "Windows source rebuild requires MSVC or macOS xwin"
    );
    cross_tools()?;
    let cache = required_env("XWIN_CACHE_DIR")?;
    let sdk = Path::new(&cache).join("xwin");
    let llvm_tool = |name: &str| match env::var_os("HONGGUO_WINDOWS_LLVM_BIN") {
        Some(directory) => find_tool(&string(PathBuf::from(directory).join(name))),
        None => find_tool(name),
    };
    let linker = match env::var("HONGGUO_WINDOWS_LINKER") {
        Ok(path) => find_tool(&path)?,
        Err(_) => llvm_tool("lld-link")?,
    };
    let includes = [
        "crt/include",
        "sdk/include/ucrt",
        "sdk/include/um",
        "sdk/include/shared",
        "sdk/include/winrt",
    ]
    .map(|s| sdk.join(s));
    let libraries =
        ["crt/lib/x86_64", "sdk/lib/ucrt/x86_64", "sdk/lib/um/x86_64"].map(|s| sdk.join(s));
    let compiler = llvm_tool("clang-cl")?;
    for path in [&compiler, &linker]
        .into_iter()
        .chain(includes.iter())
        .chain(libraries.iter())
    {
        ensure!(
            path.exists(),
            "Windows rebuild toolchain missing: {}",
            path.display()
        );
    }
    env.insert("NSH_WINDOWS_CLANG".into(), string(&compiler));
    env.insert(
        "NSH_WINDOWS_RC".into(),
        string(match env::var("HONGGUO_WINDOWS_RC") {
            Ok(path) => find_tool(&path)?,
            Err(_) => llvm_tool("llvm-rc")?,
        }),
    );
    let flags = format!(
        "--target=x86_64-pc-windows-msvc {}",
        includes
            .iter()
            .map(|p| format!("-imsvc\"{}\"", p.display()))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let link = libraries
        .iter()
        .map(|p| format!("/LIBPATH:\"{}\"", p.display()))
        .collect::<Vec<_>>()
        .join(" ");
    options.extend(words(&[
        "-DCMAKE_SYSTEM_NAME=Windows",
        "-DCMAKE_SYSTEM_PROCESSOR=AMD64",
    ]));
    let tools = absolute("target/nsh-cross-tools")?;
    for (key, value) in [
        ("CMAKE_C_COMPILER", compiler.clone()),
        ("CMAKE_CXX_COMPILER", compiler),
        ("CMAKE_AR", llvm_tool("llvm-lib")?),
        ("CMAKE_LINKER", linker),
        ("CMAKE_MT", llvm_tool("llvm-mt")?),
        ("CMAKE_RC_COMPILER", tools.join("nsh-windows-rc")),
        ("CMAKE_C_COMPILER_LAUNCHER", tools.join("nsh-windows-clang")),
        (
            "CMAKE_CXX_COMPILER_LAUNCHER",
            tools.join("nsh-windows-clang"),
        ),
    ] {
        options.push(format!("-D{key}={}", value.display()));
    }
    for (key, value) in [
        ("CMAKE_C_FLAGS", &flags),
        ("CMAKE_CXX_FLAGS", &flags),
        ("CMAKE_EXE_LINKER_FLAGS", &link),
        ("CMAKE_SHARED_LINKER_FLAGS", &link),
    ] {
        options.push(format!("-D{key}={value}"));
    }
    Ok(options)
}
