//! Validate the shipped theme and its Rust consumers without running an app or touching user data.
use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde_json::{Map, Value};
use std::{collections::BTreeSet, fs, path::Path};

pub(crate) fn check() -> Result<()> {
    let root = Path::new("crates/hongguo-nsh");
    let document: Value = serde_json::from_str(&fs::read_to_string(root.join("theme.json"))?)?;
    let light = document["cssVars"]["light"]
        .as_object()
        .context("Missing light theme")?;
    let dark = document["cssVars"]["dark"]
        .as_object()
        .context("Missing dark theme")?;
    let keys = |theme: &Map<String, Value>| {
        theme
            .keys()
            .filter(|key| key.as_str() != "radius")
            .cloned()
            .collect::<BTreeSet<_>>()
    };
    ensure!(keys(light) == keys(dark), "Light/dark color roles differ");
    for (name, theme) in [("light", light), ("dark", dark)] {
        for role in theme.keys() {
            ensure!(
                !["video-picker", "sidebar", "brand-action", "native-range"]
                    .iter()
                    .any(|prefix| role.starts_with(prefix))
                    && !["field", "video-sheet", "video-series-bar", "video-heat"]
                        .contains(&role.as_str()),
                "{name}: redundant feature palette {role}; use canonical theme roles"
            );
        }
        for (foreground, background) in [
            ("foreground", "background"),
            ("card-foreground", "card"),
            ("popover-foreground", "popover"),
            ("secondary-foreground", "secondary"),
            ("accent-foreground", "accent"),
            ("primary-foreground", "primary"),
            ("selection-foreground", "selection"),
            ("muted-foreground", "background"),
            ("muted-foreground", "popover"),
            ("muted-foreground", "muted"),
        ] {
            let a = luminance(theme, foreground)?;
            let b = luminance(theme, background)?;
            let contrast = (a.max(b) + 0.05) / (a.min(b) + 0.05);
            ensure!(
                contrast >= 4.5,
                "{name}: {foreground}/{background} contrast {contrast:.2}:1 is below 4.5:1"
            );
        }
    }
    let references = Regex::new(r#"\.color\(\s*"([a-z-]+)""#)?;
    check_consumers(&root.join("src"), light, &references)?;
    println!("Theme roles, paired contrast and Rust color references verified (light + dark)");
    Ok(())
}

fn check_consumers(path: &Path, roles: &Map<String, Value>, references: &Regex) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let path = entry?.path();
        if path.is_dir() {
            check_consumers(&path, roles, references)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let source = fs::read_to_string(&path)?;
            for capture in references.captures_iter(&source) {
                let role = &capture[1];
                ensure!(
                    roles.contains_key(role),
                    "{}: unknown theme role {role}",
                    path.display()
                );
                ensure!(
                    !role.starts_with("selection") || path.ends_with("ui/palette.rs"),
                    "{}: selection must consume ui::palette's paired state",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

fn luminance(theme: &Map<String, Value>, role: &str) -> Result<f64> {
    let hex = theme
        .get(role)
        .and_then(Value::as_str)
        .context(format!("Missing role {role}"))?;
    ensure!(
        hex.starts_with('#') && hex.len() == 7,
        "{role}: contrast roles require opaque #RRGGBB"
    );
    let rgb = u32::from_str_radix(&hex[1..], 16)?;
    let linear = |shift: u32| {
        let channel = f64::from((rgb >> shift) & 255_u32) / 255.0;
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    Ok(0.2126 * linear(16) + 0.7152 * linear(8) + 0.0722 * linear(0))
}
