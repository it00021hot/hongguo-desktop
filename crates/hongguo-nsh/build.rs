use std::{env, fmt::Write, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("crate root"));
    windows_resources(&root);
    let source = root.join("../../src/utils/danmaku-emoji-files.generated.ts");
    println!("cargo:rerun-if-changed={}", source.display());
    let table = fs::read_to_string(&source).expect("read shared emoji catalog");
    let mut generated = String::from("pub const SYSTEM: &[SystemEmoji] = &[\n");
    let mut count = 0;
    for line in table
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("['"))
    {
        let row = line
            .strip_prefix("['")
            .and_then(|line| line.strip_suffix("'],"))
            .expect("generated emoji row");
        let (code, file) = row.split_once("', '").expect("emoji code and filename");
        assert!(
            file.ends_with(".webp") && !file.contains(['/', '\\']),
            "emoji filename"
        );
        let path = root.join("../../src/assets/emoji").join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = fs::read(path).expect("read shared emoji asset");
        let mut uri = String::from("data:image/webp,");
        for byte in bytes {
            write!(uri, "%{byte:02X}").expect("write emoji URI");
        }
        writeln!(generated, "SystemEmoji {{ code: {code:?}, uri: {uri:?} }},")
            .expect("write catalog");
        count += 1;
    }
    assert!(count > 0, "empty emoji catalog");
    generated.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").expect("build output")).join("emoji_catalog.rs"),
        generated,
    )
    .expect("write embedded emoji catalog");
}

fn windows_resources(root: &std::path::Path) {
    if env::var("CARGO_CFG_TARGET_OS").expect("target OS") != "windows" {
        return;
    }
    for path in [
        root.join("windows.rc"),
        root.join("windows.manifest"),
        root.join("../hongguo-tauri-app/icons/icon.ico"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    embed_resource::compile_for(
        root.join("windows.rc"),
        ["hongguo-nsh"],
        embed_resource::NONE,
    )
    .manifest_required()
    .expect("compile required Windows application icon and manifest");
}
