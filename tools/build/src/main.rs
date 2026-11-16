mod android;
mod codec;
mod codec_export;
mod enhancement;
mod evidence;
mod host;
mod io;
mod media;
mod pe;
mod release;
mod relink;
mod sdk;
mod signing;

use anyhow::{Result, bail};
use std::{env, path::Path};

fn main() -> Result<()> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let invoked = env::current_exe()?;
    let name = invoked.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    match name {
        "native-media-link" => return relink::capture(&args),
        "nsh-windows-rc" | "rc" => return codec::resource_compiler(&args),
        "nsh-windows-clang" => return codec::clang_launcher(&args),
        _ => {}
    }
    if args.is_empty() || args[0] == "--help" {
        println!(
            "hongguo-build <setup|build|stage|release-preflight|release-assets|codec-build|sdk-package|enhancement-assets|relink|resign-ios|cross-tools|reuse-runtime> [arguments]"
        );
        return Ok(());
    }
    let action = args.remove(0);
    match action.as_str() {
        "relink" => return relink::replay(&args),
        "resign-ios" => return signing::resign(&args),
        _ => {}
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    env::set_current_dir(&root)?;
    match action.as_str() {
        "setup" => {
            sdk::prepare(
                args.first()
                    .map(String::as_str)
                    .unwrap_or(&io::rust_target()?),
            )?;
        }
        "build" => host::build(&args)?,
        "stage" => host::stage(&args)?,
        "release-preflight" => release::preflight(&args)?,
        "release-assets" => release::assets(&args)?,
        "codec-build" => codec::build(&args)?,
        "sdk-package" => codec_export::package(&args)?,
        "enhancement-assets" => enhancement::check(&args)?,
        "cross-tools" => codec::cross_tools()?,
        "reuse-runtime" => {
            anyhow::ensure!(
                Path::new("target/native-runtime/lib/arm64-v8a/libnsh.so").is_file(),
                "Missing replacement Android runtime"
            );
        }
        _ => bail!("Unknown build command: {action}"),
    }
    Ok(())
}
