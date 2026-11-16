use crate::{evidence::capture, io::*};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn resign(args: &[String]) -> Result<()> {
    let values = options(args, &["--app", "--binary", "--identity", "--profile"])?;
    let app = absolute(option(&values, "--app")?)?;
    let binary = absolute(option(&values, "--binary")?)?;
    let identity = option(&values, "--identity")?;
    let info = plist(&app.join("Info.plist"))?;
    let executable = info["CFBundleExecutable"]
        .as_str()
        .context("Missing app executable")?;
    relative(executable)?;
    ensure!(!executable.contains('/'), "Invalid app executable");
    let temporary = tempfile::Builder::new().prefix("guava-resign-").tempdir()?;
    let mut args = words(&["--force", "--sign", identity, "--timestamp=none"]);
    if identity != "-" {
        let profile = option(&values, "--profile")?;
        let decoded = temporary.path().join("profile.plist");
        write(
            &decoded,
            output("security", &words(&["cms", "-D", "-i", profile]))?,
        )?;
        let expiration = capture(
            r"<key>ExpirationDate</key>\s*<date>([^<]+)</date>",
            &text(&decoded)?,
        )?;
        let expires: u64 = output(
            "/bin/date",
            &words(&["-j", "-u", "-f", "%Y-%m-%dT%H:%M:%SZ", &expiration, "+%s"]),
        )?
        .parse()?;
        ensure!(
            expires > SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            "Provisioning profile has expired"
        );
        let prefixes = extract_plist(&decoded, "ApplicationIdentifierPrefix")?;
        let prefix = prefixes[0]
            .as_str()
            .context("Profile application prefix missing")?;
        let bundle_id = info["CFBundleIdentifier"]
            .as_str()
            .context("App identifier missing")?;
        let identifier = format!("{prefix}.{bundle_id}");
        let granted = extract_plist(&decoded, "Entitlements")?;
        let pattern = granted["application-identifier"]
            .as_str()
            .context("Profile identifier missing")?;
        ensure!(
            pattern == identifier
                || pattern
                    .strip_suffix('*')
                    .is_some_and(|p| identifier.starts_with(p)),
            "Provisioning profile does not authorize {bundle_id}"
        );
        let team = granted["com.apple.developer.team-identifier"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("Profile signing team missing")?;
        let entitlements = temporary.path().join("entitlements.plist");
        write_json(
            &entitlements,
            &json!({"application-identifier":identifier,"com.apple.developer.team-identifier":team,"get-task-allow":granted["get-task-allow"] == true}),
        )?;
        run(
            "plutil",
            &words(&["-convert", "xml1", &string(&entitlements)]),
            &Environment::new(),
            None,
        )?;
        copy(profile, app.join("embedded.mobileprovision"))?;
        args.extend(words(&[
            "--entitlements",
            &string(entitlements),
            "--generate-entitlement-der",
        ]));
    } else {
        ensure!(
            info["CFBundleSupportedPlatforms"]
                .as_array()
                .is_some_and(|v| v.iter().any(|s| s == "iPhoneSimulator")),
            "Ad-hoc signing is only supported for simulator apps"
        );
    }
    copy(binary, app.join(executable))?;
    args.push(string(&app));
    run("codesign", &args, &Environment::new(), None)?;
    run(
        "codesign",
        &words(&["--verify", "--strict", &string(app)]),
        &Environment::new(),
        None,
    )
}

fn plist(path: &Path) -> Result<Value> {
    Ok(serde_json::from_str(&output(
        "plutil",
        &words(&["-convert", "json", "-o", "-", &string(path)]),
    )?)?)
}

fn extract_plist(path: &Path, key: &str) -> Result<Value> {
    Ok(serde_json::from_str(&output(
        "plutil",
        &words(&["-extract", key, "json", "-o", "-", &string(path)]),
    )?)?)
}
