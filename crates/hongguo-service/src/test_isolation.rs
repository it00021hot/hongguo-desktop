//! Only tests that mutate process-wide environment, counters or failpoints use a child.
//! The default harness and all other service tests keep their normal parallel execution.
use std::sync::atomic::{AtomicU64, Ordering};

const CHILD_TEST: &str = "HONGGUO_ISOLATED_SERVICE_TEST";
static NEXT: AtomicU64 = AtomicU64::new(1);

/// Returns true in the parent after executing this exact test in a fresh process.
pub(crate) fn run_in_child() -> bool {
    let thread = std::thread::current();
    let name = thread.name().expect("libtest names each test thread");
    if std::env::var(CHILD_TEST).ok().as_deref() == Some(name) {
        return false;
    }
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/service-test-isolation")
        .join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&directory).expect("create isolated test data root");
    let result = std::process::Command::new(std::env::current_exe().expect("service test runner"))
        .args(["--exact", name, "--nocapture"])
        .env(CHILD_TEST, name)
        .env("HONGGUO_DATA_DIR", &directory)
        .env("CARGO_TARGET_DIR", &directory)
        .env_remove(hongguo_common::offline_qa::ENV)
        .env_remove("HONGGUO_REUSE_DEVICE_JSON")
        .output();
    let _ = std::fs::remove_dir_all(directory);
    let output = result.expect("run isolated service test");
    assert!(
        output.status.success(),
        "isolated {name} failed ({}):\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}
