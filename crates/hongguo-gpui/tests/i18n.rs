#[path = "../src/i18n.rs"]
mod i18n;

fn source_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("读取 GPUI 源码目录") {
        let path = entry.expect("读取目录项").path();
        if path.is_dir() {
            source_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn literal_keys(source: &str, out: &mut std::collections::BTreeSet<String>) {
    for needle in ["crate::i18n::t(", "crate::i18n::tf("] {
        let mut rest = source;
        while let Some(pos) = rest.find(needle) {
            rest = &rest[pos + needle.len()..];
            let trimmed = rest.trim_start();
            let Some(after_quote) = trimmed.strip_prefix('"') else {
                continue;
            };
            if let Some(end) = after_quote.find('"') {
                out.insert(after_quote[..end].to_string());
            }
        }
    }
}

#[test]
fn locales_share_keys_and_switch_text() {
    let mut zh: Vec<_> = i18n::catalog(i18n::Locale::ZhCn).keys().cloned().collect();
    let mut en: Vec<_> = i18n::catalog(i18n::Locale::EnUs).keys().cloned().collect();
    zh.sort();
    en.sort();
    assert_eq!(zh, en, "zh-CN 与 en-US 的词条结构必须完全一致");

    i18n::set_locale(i18n::Locale::ZhCn);
    assert_eq!(i18n::t("nav.settings.title"), "设置");
    i18n::set_locale(i18n::Locale::EnUs);
    assert_eq!(i18n::t("nav.settings.title"), "Settings");
    assert_eq!(i18n::t("missing.key"), "missing.key");
    assert_eq!(i18n::Locale::ALL.len(), 2);
    assert_eq!(i18n::Locale::ZhCn.tag(), "zh-CN");
    assert_eq!(i18n::Locale::EnUs.label_key(), "search.langEn");
}

#[test]
fn every_literal_key_used_by_gpui_exists_in_both_locales() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    source_files(&src, &mut files);

    let mut used = std::collections::BTreeSet::new();
    for file in files {
        let source = std::fs::read_to_string(&file).expect("读取 GPUI 源码");
        literal_keys(&source, &mut used);
    }
    assert!(!used.is_empty(), "GPUI 源码里应能找到 i18n 调用");

    for locale in i18n::Locale::ALL {
        let catalog = i18n::catalog(locale);
        for key in &used {
            assert!(
                catalog.contains_key(key),
                "{} 缺少 GPUI 使用的词条 {key}",
                locale.tag()
            );
        }
    }
}
