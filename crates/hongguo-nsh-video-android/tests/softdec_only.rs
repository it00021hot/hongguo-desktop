#[test]
fn android_audio_clock_cannot_attach_a_video_surface_or_native_pixel_decoder() {
    let playback = include_str!(
        "../../hongguo-nsh-android/android/app/src/main/java/com/hongguo/nsh/PixelPlayback.java"
    );
    let output = include_str!(
        "../../hongguo-nsh-android/android/app/src/main/java/com/hongguo/nsh/PixelOutput.java"
    );
    let renderer = include_str!("../src/render.rs");
    for source in [playback, output, renderer] {
        for forbidden in ["setSurface(", "SurfaceFrames", "rawSurface", "directMode"] {
            assert!(
                !source.contains(forbidden),
                "native video bypass remains: {forbidden}"
            );
        }
    }
}
