package com.hongguo.nsh;

import java.io.IOException;
import org.json.JSONObject;

/** Rust owns the decoder, PCM device clock and presentation generation. */
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

    PixelDirect(PixelPlayback playback, long epoch, String source) throws IOException {
        this.playback = playback;
        this.epoch = epoch;
        this.handle = playback.owner.nativeHandle();
        nativeOpen(handle, epoch, source);
    }
    // Playback intent belongs to the UI; core holding gates the real PCM clock.
    boolean mayPlay() { return !closed && !ended; }
    boolean needsFrames() { return !ended && (gate.holding() || !gate.ready()); }
    void prepared() { publishLoad(); }
    void control(boolean playing, float volume, boolean muted, double rate, boolean claimOwner) throws IOException { nativeControl(handle, epoch, playing, volume, muted, rate, claimOwner); }
    JSONObject status() throws Exception { return new JSONObject(nativeStatus(handle, epoch)); }
    boolean isOwner() throws IOException { return nativeIsOwner(handle, epoch); }
    void beginSeek() {
        gate.beginSeek(); ended = false; endEmitted = false;
    }
    boolean seek(long position) throws IOException {
        long generation = nativeSeek(handle, epoch, position);
        if (generation < 0) return false;
        beginSeek();
        gate.seekCompleted(generation);
        return true;
    }
    void presented(long generation, long serial, long pts, int width, int height) {
        if (!gate.accepts(generation)) return;
        try { if (!nativePresented(handle, epoch, generation, serial)) return; }
        catch (IOException error) { playback.error("decoderFailed", "Native presentation acknowledgment failed"); return; }
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
    void emptyVideo() {
        try {
            if (!isOwner() || !status().getBoolean("videoGap")) return;
            gate.emptyVideo();
            publishBuffering();
            publishLoad();
            playback.seeks.videoComplete();
            playback.reapplyControls();
        } catch (Exception error) { playback.error("decoderFailed", "Empty movie interval could not be confirmed"); }
    }
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
        nativeClose(handle, epoch);
    }
    private static native void nativeOpen(long handle, long epoch, String source) throws IOException;
    private static native void nativeControl(long handle, long epoch, boolean playing, float volume, boolean muted, double rate, boolean claimOwner) throws IOException;
    private static native String nativeStatus(long handle, long epoch) throws IOException;
    private static native boolean nativeIsOwner(long handle, long epoch) throws IOException;
    private static native boolean nativePresented(long handle, long epoch, long generation, long serial) throws IOException;
    private static native long nativeSeek(long handle, long epoch, long position) throws IOException;
    private static native void nativeClose(long handle, long epoch);
}
