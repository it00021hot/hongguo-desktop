//! 输出卷剩余空间探测。

use std::path::Path;

use sysinfo::Disks;

/// 输出目录所在卷的剩余字节数。
///
/// 查不到时返回 `None` 而不是 0：调用方要能区分「没空间了」和「不知道有多少空间」，
/// 填 0 会让「空间不足」的判断永远成立。
pub fn free_space_at(path: &Path) -> Option<u64> {
    // 用 `absolute` 而不是 `canonicalize`：后者在 Windows 上返回带 `\\?\` 的
    // verbatim 路径，与 sysinfo 报的挂载点（`C:`）不是同一个 prefix 组件，
    // 拿去做前缀匹配永远不中。`absolute` 同样能把相对路径补成绝对路径。
    let path = std::path::absolute(path).ok()?;
    // 下载目录可能还没建，这种「查不到」不是错误
    if !path.exists() {
        return None;
    }

    let disks = Disks::new_with_refreshed_list();
    // 挂载点会互相嵌套（根 `/` 与 `/mnt/data`），取最长匹配的那个
    disks
        .list()
        .iter()
        .filter(|d| path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().components().count())
        .map(|d| d.available_space())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_volume_reports_free_space() {
        // 受限环境（沙箱、容器）里磁盘枚举可能整体失败，查不到不算失败
        if let Some(free) = free_space_at(&std::env::temp_dir()) {
            assert!(free > 0, "临时目录所在卷应当还有剩余空间: {free}");
        }
    }

    #[test]
    fn missing_path_has_no_free_space() {
        let missing = std::env::temp_dir().join("hg-disk-space-definitely-not-here");
        assert!(
            free_space_at(&missing).is_none(),
            "路径不存在时必须报「查不到」，不能报 0"
        );
    }
}
