package com.hongguo.nsh;

import java.io.IOException;

/** Audio stays on MediaPlayer; Rust owns every video frame and its generation. */
final class PixelDirect {
    private final PixelPlayback playback;
    private final long handle;
    private final long epoch;
    private final PixelDirectGate gate = new PixelDirectGate();
    private boolean loaded;
    private boolean ended;
    private boolean endEmitted;
    private boolean closed;
    private Boolean reportedBuffering;
    private final Runnable clock = this::tick;

    PixelDirect(PixelPlayback playback, long epoch, String source) throws IOException {
        this.playback = playback;
        this.epoch = epoch;
        this.handle = playback.owner.nativeHandle();
        nativeOpen(handle, epoch, source);
    }
    boolean mayPlay() { return gate.mayPlay() && !ended && !playback.seeks.pending(); }
    void prepared() { publishLoad(); wake(); }
    void wake() {
        playback.handler.removeCallbacks(clock);
        if (!closed && playback.prepared && playback.active()) playback.handler.post(clock);
    }
    private void tick() {
        if (closed || playback.generation != epoch || playback.player == null || !playback.prepared) return;
        try {
            nativeClock(handle, epoch, Math.max(0, playback.player.getCurrentPosition()), playback.seeks.audioPending());
            if (playback.active() && playback.player.isPlaying()) playback.handler.postDelayed(clock, 16);
        } catch (RuntimeException error) { playback.error("decoderFailed", "Audio clock is unavailable"); }
    }
    void beginSeek() {
        gate.beginSeek(); ended = false; endEmitted = false;
        if (playback.player.isPlaying()) playback.player.pause();
        playback.player.setVolume(0f, 0f);
        nativeClock(handle, epoch, Math.max(0, playback.player.getCurrentPosition()), true);
    }
    void audioSeekCompleted(long position) throws IOException {
        gate.seekCompleted(nativeSeek(handle, epoch, position));
    }
    void presented(long generation, long serial, long pts, int width, int height) {
        if (!gate.accepts(generation)) return;
        boolean changed = !gate.mayPlay();
        boolean first = !gate.ready();
        gate.presented(generation);
        publishBuffering();
        if (first) android.util.Log.i("HongguoVideo", "Direct GPU frame epoch=" + epoch + " generation=" + generation + " serial=" + serial + " ptsMs=" + pts + " size=" + width + "x" + height);
        playback.output.videoWidth = width; playback.output.videoHeight = height;
        publishLoad();
        playback.seeks.videoComplete();
        if (changed) playback.reapplyControls();
        finish();
    }
    void holding(boolean holding) {
        if (closed) return;
        boolean changed = gate.holding() != holding;
        gate.holding(holding);
        publishBuffering();
        if (changed) playback.reapplyControls();
    }
    void audioBuffering(boolean buffering) { gate.audioBuffering(buffering); publishBuffering(); }
    private void publishBuffering() {
        boolean buffering = gate.buffering();
        if (reportedBuffering == null || reportedBuffering != buffering) {
            reportedBuffering = buffering;
            playback.emit("buffer", "isBuffering", buffering);
        }
    }
    private void publishLoad() {
        if (!loaded && gate.ready() && playback.prepared) {
            loaded = true;
            playback.emit("load", "durationMs", playback.duration(), "width", playback.output.videoWidth, "height", playback.output.videoHeight);
            playback.seeks.prepared();
        }
    }
    void ended() { ended = true; finish(); }
    private void finish() {
        if (ended && gate.mayPlay() && !endEmitted && !playback.seeks.pending()) {
            endEmitted = true; playback.emit("end");
        }
    }
    void close() {
        if (closed) return;
        closed = true;
        gate.close();
        playback.handler.removeCallbacks(clock);
        nativeClose(handle, epoch);
    }
    private static native void nativeOpen(long handle, long epoch, String source) throws IOException;
    private static native void nativeClock(long handle, long epoch, long position, boolean seeking);
    private static native long nativeSeek(long handle, long epoch, long position) throws IOException;
    private static native void nativeClose(long handle, long epoch);
}
