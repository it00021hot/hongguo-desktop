#[test]
fn android_audio_clock_cannot_attach_a_video_surface_or_native_pixel_decoder() {
    let playback = include_str!(
        "../../hongguo-nsh-android/android/app/src/main/java/com/hongguo/nsh/PixelPlayback.java"
    );
    let output = include_str!(
        "../../hongguo-nsh-android/android/app/src/main/java/com/hongguo/nsh/PixelOutput.java"
    );
    let renderer = include_str!("../src/native_renderer.rs");
    for source in [playback, output, renderer] {
        for forbidden in ["setSurface(", "SurfaceFrames", "rawSurface", "directMode"] {
            assert!(
                !source.contains(forbidden),
                "native video bypass remains: {forbidden}"
            );
        }
    }
}

#[test]
fn android_playback_controls_the_shared_pcm_session_without_a_system_decoder() {
    let java = include_str!(
        "../../hongguo-nsh-android/android/app/src/main/java/com/hongguo/nsh/PixelPlayback.java"
    );
    for forbidden in [
        "android.media.MediaPlayer",
        "setDataSource(",
        "getCurrentPosition(",
    ] {
        assert!(
            !java.contains(forbidden),
            "system media dependency remains: {forbidden}"
        );
    }
    let direct = include_str!(
        "../../hongguo-nsh-android/android/app/src/main/java/com/hongguo/nsh/PixelDirect.java"
    );
    assert!(direct.contains("nativeControl"));
    assert!(direct.contains("nativeStatus"));
}
