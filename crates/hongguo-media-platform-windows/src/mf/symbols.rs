use std::ffi::CStr;

pub(super) fn resolve<T>(
    names: &[&CStr],
    mut lookup: impl FnMut(&CStr) -> Option<T>,
) -> Result<Vec<T>, String> {
    names
        .iter()
        .map(|name| lookup(name).ok_or_else(|| name.to_string_lossy().into_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_export_rejects_the_whole_optional_runtime() {
        let exports = [c"MFStartup", c"MFCreateMediaType", c"MFTEnumEx"];
        let result = resolve(&exports, |name| (name != c"MFCreateMediaType").then_some(1));
        assert_eq!(result, Err("MFCreateMediaType".into()));
    }

    #[test]
    fn all_exports_are_resolved_in_the_declared_order() {
        let exports = [c"MFStartup", c"MFCreateMediaType"];
        let result = resolve(&exports, |name| {
            Some(if name == c"MFStartup" { 10 } else { 20 })
        });
        assert_eq!(result, Ok(vec![10, 20]));
    }

    #[test]
    fn empty_table_needs_no_export_lookup() {
        let result = resolve::<usize>(&[], |_| panic!("no exports were requested"));
        assert_eq!(result, Ok(Vec::new()));
    }
}
