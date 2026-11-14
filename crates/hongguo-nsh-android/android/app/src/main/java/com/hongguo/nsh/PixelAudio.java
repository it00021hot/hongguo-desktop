package com.hongguo.nsh;

import android.content.Context;
import android.content.BroadcastReceiver;
import android.content.Intent;
import android.content.IntentFilter;
import android.os.Build;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;

/** Playback-session audio focus, matching NSH's original Android video contract. */
final class PixelAudio {
    private final PixelPlayback playback;
    private AudioFocusRequest request;
    private boolean held;
    boolean interrupted;
    private boolean registered;
    private final BroadcastReceiver noisy = new BroadcastReceiver() {
        @Override public void onReceive(Context context, Intent intent) {
            if (AudioManager.ACTION_AUDIO_BECOMING_NOISY.equals(intent.getAction())) {
                interrupted = true;
                playback.interruptAudio();
            }
        }
    };
    PixelAudio(PixelPlayback playback) {
        this.playback = playback;
        IntentFilter filter = new IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY);
        if (Build.VERSION.SDK_INT >= 33) playback.activity.registerReceiver(noisy, filter, Context.RECEIVER_NOT_EXPORTED);
        else playback.activity.registerReceiver(noisy, filter);
        registered = true;
    }
    boolean granted() { return held; }
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
            playback.interruptAudio();
        }
    }
    void abandon() {
        AudioManager manager = (AudioManager) playback.activity.getSystemService(Context.AUDIO_SERVICE);
        if (manager != null && request != null) manager.abandonAudioFocusRequest(request);
        held = false;
        interrupted = false;
    }
    void release() {
        abandon();
        if (registered) { playback.activity.unregisterReceiver(noisy); registered = false; }
    }
}
