use crate::{
    codec,
    evidence::*,
    io::*,
    pe,
    sdk::{self, Artifact},
};
use anyhow::{Context, Result, ensure};
use flate2::{Compression, GzBuilder};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

const ARCHIVE_EPOCH: u64 = 1_794_614_400;

pub fn package(args: &[String]) -> Result<()> {
    let selection = args.first().map(String::as_str).unwrap_or("all");
    let build = args.get(1).map(absolute).transpose()?;
    ensure!(
        selection != "all" || build.is_none(),
        "A VVdeC build prefix requires one native target"
    );
    let triples = if selection == "all" {
        TRIPLES.to_vec()
    } else {
        vec![selection]
    };
    let mut lock: sdk::Lock = read_json("scripts/native-sdk.sources.json")?;
    ensure!(
        lock.sdk_version == sdk::SDK_VERSION,
        "Re-export all targets when changing SDK versions"
    );
    if selection == "all" {
        lock.targets.clear();
    }
    for triple in triples {
        lock.targets
            .insert(triple.into(), package_target(triple, build.as_deref())?);
    }
    write_json("scripts/native-sdk.sources.json", &lock)
}

fn package_target(triple: &str, build: Option<&Path>) -> Result<sdk::LockedTarget> {
    let target = target(triple)?;
    let output = Path::new("vendor/native-codecs");
    fs::create_dir_all(output)?;
    let work = tempfile::Builder::new()
        .prefix(".codec-sdk-stage-")
        .tempdir_in(output)?;
    let destination = work.path().join(triple);
    fs::create_dir_all(&destination)?;
    let hevc_dir = "libde265-1.1.3";
    let vvdec_dir = "vvdec-3.2.1";
    let hevc = if build.is_some() {
        let previous = sdk::verify(triple, &Path::new("target/native-media").join(triple))?;
        copy(previous.prefix, destination.join(hevc_dir))?;
        previous.manifest["recipe"]["hevc"].clone()
    } else {
        stage_hevc(&destination.join(hevc_dir), &target)?
    };
    let vvdec = stage_vvdec(&destination.join(vvdec_dir), &target, build)?;
    let recipe = json!({"target":target,"hevc":{"source":codec::hevc_source(),"build":hevc["build"]},"vvdec":{"source":vvdec["source"],"build":vvdec["build"],"target":vvdec["target"]}});
    let recipe_sha256 = json_digest(&recipe)?;
    let sdk = json!({"schemaVersion":1,"sdkVersion":sdk::SDK_VERSION,"target":target,"recipe":recipe,"recipeSha256":recipe_sha256,"components":{"hevc":{"version":"1.1.3","sourceSha256":HEVC_SHA,"manifestPath":format!("{hevc_dir}/manifest.json")},"vvdec":{"version":"3.2.1","sourceSha256":VVDEC_SHA,"manifestPath":format!("{vvdec_dir}/manifest.json")}},"artifacts":sdk::inventory(&destination)?});
    write_json(destination.join("sdk.json"), &sdk)?;
    let asset_name = format!("hongguo-codecs-v2-{triple}.tar.gz");
    let archive = output.join(&asset_name);
    archive_sdk(&destination, &archive)?;
    copy(
        destination.join("sdk.json"),
        output.join(format!("{triple}.sdk.json")),
    )?;
    println!(
        "{triple}: exported codec SDK, {} bytes",
        fs::metadata(&archive)?.len()
    );
    Ok(sdk::LockedTarget {
        archive_path: Some(format!("vendor/native-codecs/{asset_name}")),
        url: None,
        asset_name,
        sha256: digest(&archive)?,
        byte_length: fs::metadata(&archive)?.len(),
        manifest_sha256: digest(destination.join("sdk.json"))?,
        recipe_sha256,
    })
}

fn archive_sdk(directory: &Path, output: &Path) -> Result<()> {
    sdk::inventory(directory)?;
    let parent = directory.parent().context("SDK archive parent")?;
    let temporary =
        tempfile::NamedTempFile::new_in(output.parent().context("Archive output directory")?)?;
    let gzip = GzBuilder::new()
        .mtime(0)
        .write(temporary.as_file(), Compression::best());
    let mut tar = tar::Builder::new(gzip);
    for path in std::iter::once(directory.to_path_buf()).chain(walk(directory)?) {
        let metadata = fs::symlink_metadata(&path)?;
        let mut header = tar::Header::new_ustar();
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(ARCHIVE_EPOCH);
        header.set_username("root")?;
        header.set_groupname("root")?;
        header.set_mode(if metadata.is_dir() {
            0o755
        } else if metadata.is_symlink() {
            0o777
        } else {
            0o644
        });
        let relative = path.strip_prefix(parent)?;
        if metadata.is_symlink() {
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            tar.append_link(&mut header, relative, fs::read_link(&path)?)?;
        } else if metadata.is_dir() {
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            tar.append_data(&mut header, relative, std::io::empty())?;
        } else {
            header.set_entry_type(tar::EntryType::Regular);
            header.set_size(metadata.len());
            tar.append_data(&mut header, relative, fs::File::open(&path)?)?;
        }
    }
    tar.into_inner()?.finish()?;
    temporary.persist(output)?;
    Ok(())
}

fn validate_inventory(root: &Path, expected: &Value) -> Result<()> {
    let artifacts: Vec<Artifact> = serde_json::from_value(expected.clone())?;
    for artifact in artifacts {
        sdk::verify_bytes(
            &checked(root, &artifact.path)?,
            &artifact.sha256,
            Some(artifact.byte_length),
        )?;
    }
    Ok(())
}

fn validate_source(input: &Path, manifest: &Value) -> Result<Vec<Artifact>> {
    let recorded: Vec<Artifact> = read_json(input.join("build-info/source-inventory.json"))?;
    ensure!(
        json_digest(&recorded)?
            == manifest["source"]["vendoredTreeSha256"]
                .as_str()
                .context("VVdeC source hash missing")?,
        "VVdeC source inventory changed"
    );
    let root = Path::new("crates/hongguo-vvdec-sys/vendor/vvdec");
    let actual = sdk::inventory(root)?;
    ensure!(
        actual.len() == recorded.len(),
        "VVdeC source tree size changed"
    );
    let names: HashMap<_, _> = actual.iter().map(|a| (&a.path, a)).collect();
    for artifact in &recorded {
        let actual = names
            .get(&artifact.path)
            .context("VVdeC source file removed")?;
        ensure!(
            actual.sha256 == artifact.sha256 && actual.byte_length == artifact.byte_length,
            "VVdeC source differs: {}",
            artifact.path
        );
    }
    Ok(recorded)
}

fn stage_vvdec(destination: &Path, target: &Target, build: Option<&Path>) -> Result<Value> {
    let input = Path::new("target/native-media")
        .join(&target.triple)
        .join("vvdec-3.2.1");
    let manifest: Value = read_json(input.join("manifest.json"))?;
    ensure!(
        manifest["source"]["version"] == "3.2.1"
            && manifest["source"]["sha256"] == VVDEC_SHA
            && manifest["target"]["triple"] == target.triple,
        "VVdeC input source or target mismatch"
    );
    let inventory = validate_source(&input, &manifest)?;
    if let Some(build) = build {
        return stage_built_vvdec(destination, target, build, &input, &inventory);
    }
    if let Some(patches) = manifest["source"]["patches"].as_array() {
        for patch in patches {
            let name = patch["path"].as_str().context("VVdeC patch name")?;
            ensure!(
                ["private-frame-allocator.patch", "picture-pool.cmake"].contains(&name)
                    && digest(Path::new("crates/hongguo-vvdec-sys").join(name))?
                        == patch["sha256"].as_str().context("Patch SHA-256")?,
                "VVdeC allocator recipe differs"
            );
        }
    }
    validate_inventory(&input, &manifest["artifacts"])?;
    let inspection = manifest["build"]["inspectionPath"]
        .as_str()
        .context("VVdeC inspection path")?;
    validate_vvdec(
        &vvdec_target(&target.triple)?,
        &read_json(checked(&input, inspection)?)?,
    )?;
    copy(input, destination)?;
    Ok(manifest)
}

fn stage_built_vvdec(
    destination: &Path,
    target: &Target,
    prefix: &Path,
    inherited: &Path,
    inventory: &Vec<Artifact>,
) -> Result<Value> {
    ensure!(
        target.triple == "aarch64-apple-darwin"
            && cfg!(target_os = "macos")
            && env::consts::ARCH == "aarch64",
        "VVdeC allocator SDK maintenance requires native Apple Silicon"
    );
    let directory = absolute("crates/hongguo-vvdec-sys")?;
    let source = directory.join("vendor/vvdec");
    let build = prefix.join("build");
    let cache = text(build.join("CMakeCache.txt"))?;
    let cached = |key: &str| capture(&format!(r"(?m)^{key}:[^=]+=(.*)$"), &cache);
    ensure!(
        cached("CMAKE_BUILD_TYPE")? == "Release"
            && Path::new(&cached("CMAKE_HOME_DIRECTORY")?).canonicalize()?
                == source.canonicalize()?
            && Path::new(&cached("CMAKE_PROJECT_INCLUDE")?).canonicalize()?
                == directory.join("picture-pool.cmake").canonicalize()?
            && Path::new(&cached("HONGGUO_PICTURE_POOL_SOURCE")?).canonicalize()?
                == prefix.join("PicListManager.cpp").canonicalize()?,
        "VVdeC build does not match pinned allocator recipe"
    );
    let info = destination.join("build-info");
    let patches = destination.join("patches");
    fs::create_dir_all(&info)?;
    fs::create_dir_all(&patches)?;
    let patched = info.join("PicListManager.cpp");
    copy(
        source.join("source/Lib/CommonLib/PicListManager.cpp"),
        &patched,
    )?;
    let patch = directory.join("private-frame-allocator.patch");
    run(
        "/usr/bin/patch",
        &words(&["--batch", "--forward", &string(&patched), &string(&patch)]),
        &Environment::new(),
        None,
    )?;
    ensure!(
        digest(patched)? == digest(prefix.join("PicListManager.cpp"))?,
        "VVdeC compiled source differs from allocator patch"
    );
    run(
        "cmake",
        &words(&[
            "--build",
            &string(&build),
            "--target",
            "install",
            "--config",
            "Release",
            "--clean-first",
        ]),
        &Environment::new(),
        None,
    )?;
    let library = prefix.join("lib/libvvdec.a");
    let inspection = output(
        tool("llvm-readobj"),
        &words(&["--file-headers", "--macho-version-min", &string(&library)]),
    )?;
    let mut arches = captures(r"(?m)^Arch: (.+)$", &inspection)?;
    arches.sort();
    arches.dedup();
    let mut platforms = captures(r"(?m)^\s+Platform: (.+)$", &inspection)?;
    platforms.sort();
    platforms.dedup();
    let mut versions = captures(r"(?m)^\s+Version: (.+)$", &inspection)?;
    versions.sort();
    versions.dedup();
    let flags_path = build.join("source/Lib/vvdec/CMakeFiles/vvdec.dir/flags.make");
    let flags = text(&flags_path)?;
    let generic = capture(r"(?m)^CXX_FLAGS = (.*)$", &flags)?;
    ensure!(
        matches(r"(?:^|\s)-mcpu=apple-m1(?:\s|$)", &generic)?,
        "VVdeC Apple Silicon objects require M1 baseline"
    );
    let evidence = json!({"configuration":"Release","sourceMatches":true,"genericFlags":generic,"architecture":if arches == ["aarch64"] { "arm64".into() } else { arches.join(",") },"platforms":platforms.iter().map(|s| if s == "macos" { "1" } else { s }).collect::<Vec<_>>(),"minimumVersions":versions});
    validate_vvdec(&vvdec_target(&target.triple)?, &evidence)?;
    copy(prefix.join("include"), destination.join("include"))?;
    copy(library, destination.join("lib/libvvdec.a"))?;
    copy(prefix.join("bindings.rs"), destination.join("bindings.rs"))?;
    copy(inherited.join("licenses"), destination.join("licenses"))?;
    for name in ["LICENSE.txt", "AUTHORS.md"] {
        copy(source.join(name), destination.join("licenses").join(name))?;
    }
    copy(directory.join("SOURCE.md"), destination.join("SOURCE.md"))?;
    for name in ["private-frame-allocator.patch", "picture-pool.cmake"] {
        copy(directory.join(name), patches.join(name))?;
    }
    copy(build.join("CMakeCache.txt"), info.join("CMakeCache.txt"))?;
    copy(flags_path, info.join("compiler-configuration.txt"))?;
    write_json(info.join("library-inspection.json"), &evidence)?;
    write_json(info.join("source-inventory.json"), inventory)?;
    let mut target = vvdec_target(&target.triple)?;
    target.cpu_baseline = "apple-m1".into();
    let manifest = json!({"schemaVersion":1,"source":{"version":"3.2.1","sha256":VVDEC_SHA,"vendoredTreeSha256":json_digest(inventory)?,"patches":sdk::inventory(&patches)?},"target":target,"bindingsPath":"bindings.rs","libraryPath":"lib/libvvdec.a","build":{"provenance":"cargo-source-build","cmakeCacheSha256":digest(info.join("CMakeCache.txt"))?,"flagsSha256":digest(info.join("compiler-configuration.txt"))?,"inspectionPath":"build-info/library-inspection.json"},"artifacts":sdk::inventory(destination)?});
    write_json(destination.join("manifest.json"), &manifest)?;
    Ok(manifest)
}

fn stage_hevc(destination: &Path, target: &Target) -> Result<Value> {
    let pointer: Value =
        read_json(Path::new("target/native-codec-build").join(format!("{}.json", target.triple)))?;
    let prefix = PathBuf::from(pointer["install"].as_str().context("HEVC install prefix")?);
    let evidence: Value = read_json(prefix.join("build-evidence.json"))?;
    ensure!(
        evidence["source"]["sha256"] == HEVC_SHA
            && evidence["source"]["commit"] == HEVC_COMMIT
            && evidence["target"]["triple"] == target.triple
            && evidence["target"]["crt"] == target.crt,
        "HEVC build source or target mismatch"
    );
    let archive = evidence["sourceArchive"]
        .as_str()
        .context("HEVC source archive")?;
    sdk::verify_bytes(Path::new(archive), HEVC_SHA, None)?;
    let build = Path::new(
        evidence["buildDirectory"]
            .as_str()
            .context("HEVC build directory")?,
    );
    let commands: Value = read_json(build.join("compile_commands.json"))?;
    let (library, runtime) = match target.os.as_str() {
        "windows" => ("lib/de265.lib", Some("bin/libde265.dll")),
        "macos" => ("lib/libde265.dylib", Some("lib/libde265.0.dylib")),
        _ => ("lib/libde265.a", None),
    };
    let image = checked(&prefix, runtime.unwrap_or(library))?;
    let mut args = words(&["--file-headers"]);
    if ["macos", "ios"].contains(&target.os.as_str()) {
        args.push("--macho-version-min".into());
    }
    args.push(string(&image));
    let inspection = output(tool("llvm-readobj"), &args)?;
    validate_hevc(target, &inspection, &commands)?;
    let imports = if target.os == "windows" {
        pe::imports(&fs::read(&image)?, true)?
    } else {
        Vec::new()
    };
    copy(prefix.join("include"), destination.join("include"))?;
    for folder in ["bin", "lib"] {
        let source = prefix.join(folder);
        if !source.exists() {
            continue;
        }
        for path in list(&source)? {
            if !path
                .extension()
                .is_some_and(|e| ["dll", "lib", "a", "dylib"].iter().any(|s| e == *s))
            {
                continue;
            }
            let name = path.file_name().context("HEVC library filename")?;
            checked(&source, &name.to_string_lossy())?;
            copy(&path, destination.join(folder).join(name))?;
        }
    }
    copy(archive, destination.join("sources/libde265-1.1.3.tar.gz"))?;
    let source = Path::new(
        evidence["sourceDirectory"]
            .as_str()
            .context("HEVC source directory")?,
    );
    for name in ["COPYING", "AUTHORS"] {
        copy(source.join(name), destination.join("licenses").join(name))?;
    }
    copy(
        prefix.join("build-evidence.json"),
        destination.join("build-info/build-evidence.json"),
    )?;
    for name in ["CMakeCache.txt", "compile_commands.json"] {
        copy(build.join(name), destination.join("build-info").join(name))?;
    }
    write_json(
        destination.join("build-info/library-inspection.json"),
        &json!({"inspection":inspection,"imports":imports}),
    )?;
    let mut manifest = json!({"schemaVersion":1,"source":codec::hevc_source(),"target":target,"libraryPath":library,"build":{"options":evidence["options"],"compiler":evidence["compiler"]},"artifacts":sdk::inventory(destination)?});
    if let Some(runtime) = runtime {
        manifest["runtimePath"] = runtime.into();
    }
    write_json(destination.join("manifest.json"), &manifest)?;
    Ok(manifest)
}
