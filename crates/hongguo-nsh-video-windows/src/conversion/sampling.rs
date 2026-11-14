pub(super) fn chroma_phase(location: Option<u8>, step: [f32; 2]) -> [f32; 2] {
    let center = [(step[0] - 1.0) * 0.5, (step[1] - 1.0) * 0.5];
    match location {
        Some(1) => [0.0, center[1]],
        Some(3) => [0.0, 0.0],
        Some(4) => [center[0], 0.0],
        Some(5) => [0.0, step[1] - 1.0],
        Some(6) => [center[0], step[1] - 1.0],
        _ => center,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_and_unspecified_chroma_position_match_the_common_gpu_center_default() {
        for location in [None, Some(0), Some(2), Some(255)] {
            assert_eq!(chroma_phase(location, [2.0, 2.0]), [0.5, 0.5]);
            assert_eq!(chroma_phase(location, [2.0, 1.0]), [0.5, 0.0]);
            assert_eq!(chroma_phase(location, [1.0, 1.0]), [0.0, 0.0]);
        }
    }
    #[test]
    fn explicit_cicp_locations_keep_their_documented_luma_phase() {
        for (location, expected) in [
            (1, [0.0, 0.5]),
            (3, [0.0, 0.0]),
            (4, [0.5, 0.0]),
            (5, [0.0, 1.0]),
            (6, [0.5, 1.0]),
        ] {
            assert_eq!(chroma_phase(Some(location), [2.0, 2.0]), expected);
        }
    }
}
