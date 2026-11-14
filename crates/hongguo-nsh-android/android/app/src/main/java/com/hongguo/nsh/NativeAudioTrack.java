package com.hongguo.nsh;

import android.media.AudioAttributes;
import android.media.MediaPlayer;
import android.media.PlaybackParams;
import java.io.IOException;

public final class NativeAudioTrack implements AutoCloseable {
    private final MediaPlayer player = new MediaPlayer();
    public volatile boolean seeking;
    public volatile boolean buffering;
    public volatile boolean ended;
    private volatile String failure;
    private float appliedRate = Float.NaN;
    private Long pendingSeek;
    private boolean closed;

    NativeAudioTrack(String source) throws IOException {
        player.setAudioAttributes(new AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_MEDIA)
            .setContentType(AudioAttributes.CONTENT_TYPE_MOVIE).build());
        player.setOnSeekCompleteListener(ignored -> seekComplete());
        player.setOnCompletionListener(ignored -> ended = true);
        player.setOnErrorListener((ignored, what, extra) -> {
            failure = "MediaPlayer error " + what + "/" + extra;
            return true;
        });
        player.setOnInfoListener((ignored, what, extra) -> {
            if (what == MediaPlayer.MEDIA_INFO_BUFFERING_START) buffering = true;
            else if (what == MediaPlayer.MEDIA_INFO_BUFFERING_END) buffering = false;
            return false;
        });
        try {
            player.setDataSource(source);
            player.prepare();
        } catch (IOException | RuntimeException error) {
            player.release();
            throw error;
        }
    }

    public double positionSeconds() {
        check();
        return player.getCurrentPosition() / 1000.0;
    }

    public void control(boolean playing, float volume, boolean muted, double rate) {
        check();
        float level = muted ? 0 : volume;
        player.setVolume(level, level);
        if (!playing) {
            if (player.isPlaying()) player.pause();
            return;
        }
        if (appliedRate != (float) rate) {
            player.setPlaybackParams(new PlaybackParams()
                .setAudioFallbackMode(PlaybackParams.AUDIO_FALLBACK_MODE_FAIL)
                .setPitch(1).setSpeed((float) rate));
            appliedRate = (float) rate;
        }
        if (!player.isPlaying()) player.start();
    }

    public synchronized void seek(double seconds) {
        check();
        pendingSeek = Math.round(seconds * 1000);
        ended = false;
        if (!seeking) beginSeek();
    }

    private void beginSeek() {
        long target = pendingSeek;
        pendingSeek = null;
        seeking = true;
        player.seekTo(target, MediaPlayer.SEEK_CLOSEST);
    }

    private synchronized void seekComplete() {
        if (closed) return;
        if (pendingSeek != null) beginSeek();
        else seeking = false;
    }

    private void check() {
        if (failure != null) throw new IllegalStateException(failure);
    }

    @Override public synchronized void close() {
        if (closed) return;
        closed = true;
        pendingSeek = null;
        player.release();
    }
}
