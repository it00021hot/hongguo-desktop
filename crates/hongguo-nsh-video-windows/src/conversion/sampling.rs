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
