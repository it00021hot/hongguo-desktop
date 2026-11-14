#[path = "../../../scripts/native-sdk-build.rs"]
#[expect(
    dead_code,
    reason = "The integration test exercises the pure verifier without Cargo environment setup"
)]
mod native_sdk;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

const TARGET: &str = "aarch64-apple-darwin";
const SDK_VERSION: &str = "native-codecs-v2-20261114";
const HEVC_SHA: &str = "7ffe3220bba8282cc6d4a2bcf36ac1b6d2899f402c335f7df377317414a025b1";
const VVDEC_SHA: &str = "5c334557a33cd93e981b84ba0e77126ef970a38e2481b67f0475db40f75379b8";

struct Fixture {
    directory: PathBuf,
    root: PathBuf,
    lock: PathBuf,
    manifest: Value,
}

impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "hongguo-sdk-{}-{stamp}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let root = directory.join("sdk");
        fs::create_dir_all(&root).unwrap();
        let hevc = root.join("libde265-1.1.3");
        let vvdec = root.join("vvdec-3.2.1");
        fs::create_dir_all(hevc.join("include/libde265")).unwrap();
        fs::create_dir_all(hevc.join("lib")).unwrap();
        fs::create_dir_all(vvdec.join("lib")).unwrap();
        fs::write(
            hevc.join("include/libde265/de265.h"),
            "synthetic codec header",
        )
        .unwrap();
        fs::write(hevc.join("lib/libde265.dylib"), "synthetic HEVC library").unwrap();
        fs::write(hevc.join("lib/libde265.0.dylib"), "synthetic HEVC library").unwrap();
        fs::write(vvdec.join("bindings.rs"), "pub const TEST: u32 = 1;").unwrap();
        fs::write(vvdec.join("lib/libvvdec.a"), "synthetic archive").unwrap();
        let target = json!({"triple":TARGET,"crt":"system"});
        let hevc_manifest = json!({"schemaVersion":1,"source":{"version":"1.1.3","sha256":HEVC_SHA},"target":target,"libraryPath":"lib/libde265.dylib","runtimePath":"lib/libde265.0.dylib","artifacts":[artifact(&hevc,"include/libde265/de265.h"),artifact(&hevc,"lib/libde265.dylib"),artifact(&hevc,"lib/libde265.0.dylib")]});
        fs::write(
            hevc.join("manifest.json"),
            serde_json::to_vec(&hevc_manifest).unwrap(),
        )
        .unwrap();
        let vvdec_manifest = json!({"schemaVersion":1,"source":{"version":"3.2.1","sha256":VVDEC_SHA},"target":target,"bindingsPath":"bindings.rs","libraryPath":"lib/libvvdec.a","artifacts":[artifact(&vvdec,"bindings.rs"),artifact(&vvdec,"lib/libvvdec.a")]});
        fs::write(
            vvdec.join("manifest.json"),
            serde_json::to_vec(&vvdec_manifest).unwrap(),
        )
        .unwrap();
        let manifest = json!({"schemaVersion":1,"sdkVersion":SDK_VERSION,"target":target,"recipeSha256":"a".repeat(64),"components":{"hevc":{"version":"1.1.3","sourceSha256":HEVC_SHA,"manifestPath":"libde265-1.1.3/manifest.json"},"vvdec":{"version":"3.2.1","sourceSha256":VVDEC_SHA,"manifestPath":"vvdec-3.2.1/manifest.json"}},"artifacts":[artifact(&root,"libde265-1.1.3/manifest.json"),artifact(&root,"libde265-1.1.3/include/libde265/de265.h"),artifact(&root,"libde265-1.1.3/lib/libde265.dylib"),artifact(&root,"libde265-1.1.3/lib/libde265.0.dylib"),artifact(&root,"vvdec-3.2.1/manifest.json"),artifact(&root,"vvdec-3.2.1/bindings.rs"),artifact(&root,"vvdec-3.2.1/lib/libvvdec.a")]});
        let mut fixture = Self {
            lock: directory.join("lock.json"),
            directory,
            root,
            manifest,
        };
        fixture.save();
        fixture
    }

    fn save(&mut self) {
        let bytes = serde_json::to_vec(&self.manifest).unwrap();
        fs::write(self.root.join("sdk.json"), &bytes).unwrap();
        let lock = json!({"schemaVersion":1,"sdkVersion":SDK_VERSION,"targets":{TARGET:{"manifestSha256":hash(&bytes),"recipeSha256":"a".repeat(64)}}});
        fs::write(&self.lock, serde_json::to_vec(&lock).unwrap()).unwrap();
    }

    fn check(&self) -> Result<(), String> {
        native_sdk::verify_sdk(&self.root, TARGET, &self.lock)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn artifact(root: &std::path::Path, relative: &str) -> Value {
    let bytes = fs::read(root.join(relative)).unwrap();
    json!({"path":relative,"sha256":hash(&bytes),"byteLength":bytes.len()})
}

#[test]
fn locked_sdk_accepts_intact_target_libraries_and_bindings() {
    assert_eq!(Fixture::new().check(), Ok(()));
}

#[test]
fn changing_a_native_library_is_rejected() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("vvdec-3.2.1/lib/libvvdec.a"),
        "modified archive",
    )
    .unwrap();
    assert!(fixture.check().is_err());
}

#[test]
fn rewriting_local_manifest_hashes_cannot_replace_the_repository_lock() {
    let mut fixture = Fixture::new();
    fixture.manifest["recipeSha256"] = json!("b".repeat(64));
    fs::write(
        fixture.root.join("sdk.json"),
        serde_json::to_vec(&fixture.manifest).unwrap(),
    )
    .unwrap();
    assert!(fixture.check().is_err());
}

#[test]
fn an_authenticated_archive_for_another_target_is_rejected() {
    let mut fixture = Fixture::new();
    fixture.manifest["target"]["triple"] = json!("x86_64-pc-windows-msvc");
    fixture.save();
    assert!(fixture.check().is_err());
}

#[test]
fn empty_artifact_inventory_is_rejected() {
    let mut fixture = Fixture::new();
    fixture.manifest["artifacts"] = json!([]);
    fixture.save();
    assert!(fixture.check().is_err());
}

#[test]
fn artifact_paths_cannot_escape_the_sdk_directory() {
    let mut fixture = Fixture::new();
    fs::write(fixture.directory.join("outside"), "private fixture").unwrap();
    fixture.manifest["artifacts"][0] = artifact(&fixture.root, "../outside");
    fixture.save();
    assert!(fixture.check().is_err());
}

#[cfg(unix)]
#[test]
fn symlink_artifacts_cannot_escape_the_sdk_directory() {
    let fixture = Fixture::new();
    let library = fixture.root.join("vvdec-3.2.1/lib/libvvdec.a");
    let outside = fixture.directory.join("outside.a");
    fs::rename(&library, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &library).unwrap();
    assert!(fixture.check().is_err());
}
