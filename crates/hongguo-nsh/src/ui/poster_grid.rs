pub(crate) fn columns(width: f32) -> u32 {
    if width >= 1536.0 {
        8
    } else if width >= 1024.0 {
        5
    } else if width >= 768.0 {
        4
    } else if width >= 640.0 {
        3
    } else {
        2
    }
}
