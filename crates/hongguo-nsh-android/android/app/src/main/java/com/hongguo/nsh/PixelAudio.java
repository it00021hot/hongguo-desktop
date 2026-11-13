package com.hongguo.nsh;

import android.content.Context;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;

/** Playback-session audio focus, matching NSH's original Android video contract. */
final class PixelAudio {
    private final PixelPlayback playback;
    private AudioFocusRequest request;
    private boolean held;
    boolean interrupted;
    PixelAudio(PixelPlayback playback) { this.playback = playback; }
    void update(boolean wanted) {
        if (!wanted) { abandon(); return; }
        if (held) return;
        AudioManager manager = (AudioManager) playback.activity.getSystemService(Context.AUDIO_SERVICE);
        if (manager == null) return;
        if (request == null) request = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
            .setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MOVIE).build())
            .setWillPauseWhenDucked(true).setOnAudioFocusChangeListener(this::changed, playback.handler).build();
        held = manager.requestAudioFocus(request) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED;
    }
    private void changed(int change) {
        if (change == AudioManager.AUDIOFOCUS_GAIN) {
            held = true;
            interrupted = false;
            playback.reapplyControls();
        } else {
            interrupted = true;
            if (change == AudioManager.AUDIOFOCUS_LOSS) held = false;
            try { if (playback.player != null && playback.prepared && playback.player.isPlaying()) playback.player.pause(); }
            catch (RuntimeException error) { playback.error("decoderFailed", "MediaPlayer audio interruption failed"); }
        }
    }
    void abandon() {
        AudioManager manager = (AudioManager) playback.activity.getSystemService(Context.AUDIO_SERVICE);
        if (manager != null && request != null) manager.abandonAudioFocusRequest(request);
        held = false;
        interrupted = false;
    }
}
