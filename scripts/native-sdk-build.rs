use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Component, Path, PathBuf},
};

const SDK_VERSION: &str = "native-codecs-v2-20261114";
const HEVC_VERSION: &str = "1.1.3";
const HEVC_SHA: &str = "7ffe3220bba8282cc6d4a2bcf36ac1b6d2899f402c335f7df377317414a025b1";
const VVDEC_VERSION: &str = "3.2.1";
const VVDEC_SHA: &str = "5c334557a33cd93e981b84ba0e77126ef970a38e2481b67f0475db40f75379b8";
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ARTIFACTS: usize = 16_384;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lock {
    schema_version: u32,
    sdk_version: String,
    targets: HashMap<String, LockedTarget>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockedTarget {
    manifest_sha256: String,
    recipe_sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sdk {
    schema_version: u32,
    sdk_version: String,
    target: Target,
    recipe_sha256: String,
    components: Components,
    artifacts: Vec<Artifact>,
}

#[derive(Deserialize)]
struct Target {
    triple: String,
    crt: Option<String>,
}

#[derive(Deserialize)]
struct Components {
    hevc: ComponentManifest,
    vvdec: ComponentManifest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ComponentManifest {
    version: String,
    source_sha256: String,
    manifest_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryManifest {
    schema_version: u32,
    source: Source,
    target: Target,
    bindings_path: Option<String>,
    library_path: Option<String>,
    runtime_path: Option<String>,
    artifacts: Vec<Artifact>,
}

#[derive(Deserialize)]
struct Source {
    version: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Artifact {
    path: String,
    sha256: String,
    byte_length: u64,
}

pub fn require_sdk(crate_directory: &Path, target: &str) -> PathBuf {
    println!("cargo:rerun-if-env-changed=HONGGUO_NATIVE_SDK_ROOT");
    let repository = crate_directory.join("../..");
    let root = env::var_os("HONGGUO_NATIVE_SDK_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| repository.join("target/native-media").join(target));
    let lock = repository.join("scripts/native-sdk.sources.json");
    println!("cargo:rerun-if-changed={}", lock.display());
    println!("cargo:rerun-if-changed={}", root.join("sdk.json").display());
    println!("cargo:rerun-if-changed={}", root.display());
    verify_sdk(&root, target, &lock).unwrap_or_else(|error| {
        panic!("Native SDK is unavailable or invalid at {}: {error}. Run node scripts/nsh.mjs setup, or set HONGGUO_NATIVE_SDK_ROOT to the verified target SDK directory", root.display())
    });
    if target.contains("msvc") {
        let features = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
        assert!(
            features.split(',').any(|feature| feature == "crt-static"),
            "The pinned Windows SDK requires Rust crt-static"
        );
    }
    root
}

pub fn verify_sdk(root: &Path, target: &str, lock: &Path) -> Result<(), String> {
    let lock: Lock = read_json(&read_manifest(lock)?)?;
    if lock.schema_version != 1 || lock.sdk_version != SDK_VERSION {
        return Err("repository native SDK lock version mismatch".into());
    }
    let locked = lock
        .targets
        .get(target)
        .ok_or_else(|| format!("native SDK target is not locked: {target}"))?;
    let canonical = root
        .canonicalize()
        .map_err(|error| format!("SDK directory: {error}"))?;
    let manifest = checked_path(&canonical, "sdk.json")?;
    let bytes = read_manifest(&manifest)?;
    if hash(&bytes) != locked.manifest_sha256 {
        return Err("sdk.json differs from the repository lock".into());
    }
    let sdk: Sdk = read_json(&bytes)?;
    if sdk.schema_version != 1
        || sdk.sdk_version != SDK_VERSION
        || sdk.target.triple != target
        || sdk.recipe_sha256 != locked.recipe_sha256
    {
        return Err("native SDK schema, target or recipe mismatch".into());
    }
    check_crt(&sdk.target, target)?;
    let listed = verify_artifacts(&canonical, &sdk.artifacts)?;
    verify_component(
        &canonical,
        &listed,
        &sdk.components.hevc,
        target,
        HEVC_VERSION,
        HEVC_SHA,
    )?;
    verify_component(
        &canonical,
        &listed,
        &sdk.components.vvdec,
        target,
        VVDEC_VERSION,
        VVDEC_SHA,
    )?;
    Ok(())
}

fn verify_component(
    root: &Path,
    listed: &HashSet<&str>,
    component: &ComponentManifest,
    target: &str,
    version: &str,
    source_sha: &str,
) -> Result<(), String> {
    let directory_name = if version == HEVC_VERSION {
        "libde265-1.1.3"
    } else {
        "vvdec-3.2.1"
    };
    let expected_manifest = format!("{directory_name}/manifest.json");
    if component.version != version
        || component.source_sha256 != source_sha
        || component.manifest_path != expected_manifest
        || !listed.contains(component.manifest_path.as_str())
    {
        return Err(format!(
            "native SDK component source or manifest mismatch: {directory_name}"
        ));
    }
    let path = checked_path(root, &component.manifest_path)?;
    let manifest: LibraryManifest = read_json(&read_manifest(&path)?)?;
    if manifest.schema_version != 1
        || manifest.source.version != version
        || manifest.source.sha256 != source_sha
        || manifest.target.triple != target
    {
        return Err(format!(
            "native component source or target mismatch: {directory_name}"
        ));
    }
    let directory = checked_path(root, directory_name)?;
    let component_files = verify_artifacts(&directory, &manifest.artifacts)?;
    for artifact in &manifest.artifacts {
        if !listed.contains(format!("{directory_name}/{}", artifact.path).as_str()) {
            return Err(format!(
                "component artifact is absent from locked SDK inventory: {}",
                artifact.path
            ));
        }
    }
    if version == VVDEC_VERSION {
        check_crt(&manifest.target, target)?;
        let library = if target.contains("msvc") {
            "lib/vvdec.lib"
        } else {
            "lib/libvvdec.a"
        };
        if manifest.bindings_path.as_deref() != Some("bindings.rs")
            || manifest.library_path.as_deref() != Some(library)
            || !component_files.contains("bindings.rs")
            || !component_files.contains(library)
        {
            return Err("VVdeC library or bindings are absent from the locked SDK".into());
        }
    } else {
        check_crt(&manifest.target, target)?;
        let library = if target.contains("msvc") {
            "lib/de265.lib"
        } else if target.ends_with("apple-darwin") {
            "lib/libde265.dylib"
        } else {
            "lib/libde265.a"
        };
        let runtime = if target.contains("msvc") {
            Some("bin/libde265.dll")
        } else if target.ends_with("apple-darwin") {
            Some("lib/libde265.0.dylib")
        } else {
            None
        };
        if manifest.library_path.as_deref() != Some(library)
            || !component_files.contains(library)
            || !component_files.contains("include/libde265/de265.h")
            || manifest.runtime_path.as_deref() != runtime
            || runtime.is_some_and(|path| !component_files.contains(path))
        {
            return Err("HEVC library, header or runtime is absent from the locked SDK".into());
        }
    }
    Ok(())
}

fn check_crt(metadata: &Target, target: &str) -> Result<(), String> {
    let expected = if target.contains("msvc") {
        "static"
    } else {
        "system"
    };
    if metadata.crt.as_deref() != Some(expected) {
        return Err(format!("native SDK CRT mismatch for {target}"));
    }
    Ok(())
}

fn verify_artifacts<'a>(
    root: &Path,
    artifacts: &'a [Artifact],
) -> Result<HashSet<&'a str>, String> {
    if artifacts.is_empty() || artifacts.len() > MAX_ARTIFACTS {
        return Err("native SDK artifact inventory is empty or exceeds its limit".into());
    }
    let mut listed = HashSet::new();
    for artifact in artifacts {
        if !listed.insert(artifact.path.as_str()) {
            return Err(format!(
                "native SDK artifact is duplicated: {}",
                artifact.path
            ));
        }
        let path = checked_path(root, &artifact.path)?;
        let metadata = path.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file()
            || metadata.len() != artifact.byte_length
            || hash(&fs::read(&path).map_err(|error| error.to_string())?) != artifact.sha256
        {
            return Err(format!(
                "native SDK artifact is missing or modified: {}",
                artifact.path
            ));
        }
    }
    Ok(listed)
}

fn checked_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if relative.is_empty()
        || relative.contains('\\')
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(format!("native SDK path is not relative: {relative}"));
    }
    let resolved = root
        .join(path)
        .canonicalize()
        .map_err(|error| format!("native SDK path {relative}: {error}"))?;
    if !resolved.starts_with(root) {
        return Err(format!("native SDK path escapes its directory: {relative}"));
    }
    Ok(resolved)
}

fn read_manifest(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return Err(format!(
            "native SDK manifest exceeds its size limit: {}",
            path.display()
        ));
    }
    fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn read_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("native SDK manifest JSON: {error}"))
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
