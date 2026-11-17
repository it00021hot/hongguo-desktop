use sha2::{Digest, Sha256};
use std::{env, error::Error, fs::File, io::Read, path::PathBuf};

const MATERIALS: [(&str, usize, &str); 2] = [
    (
        "image.bin",
        4_501_136,
        "c6831ab265a0cb76dacfa3b7041e9ea0f706490106a74e12f39058671cc14fe3",
    ),
    (
        "license.txt",
        304,
        "584a01183f1e6783326bff49d4f1319f33fcce678afb93ca0dc8b60a56b65855",
    ),
];

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("missing crate root")?)
        .join("../../assets/metasec/v5");
    for (name, length, sha256) in MATERIALS {
        let path = root.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let mut bytes = Vec::with_capacity(length);
        File::open(path)?
            .take(length as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() != length || hex::encode(Sha256::digest(&bytes)) != sha256 {
            return Err(format!("MetaSec asset integrity mismatch: {name}").into());
        }
    }
    Ok(())
}
