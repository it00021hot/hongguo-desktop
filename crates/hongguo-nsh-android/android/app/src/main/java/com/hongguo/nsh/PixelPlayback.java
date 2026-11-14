package com.hongguo.nsh;

import android.app.Activity;
import android.os.Handler;
import android.os.Looper;
import android.view.Choreographer;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import org.json.JSONObject;

final class PixelPlayback {
    final Activity activity;
    final PixelVideoComponent owner;
    final Handler handler = new Handler(Looper.getMainLooper());
    final PixelOutput output;
    final PixelSeeks seeks;
    final PixelAudio audio;
    PixelDirect direct;
    long generation;
    String sourceId = "";
    boolean prepared;
    private JSONObject props;
    private volatile boolean released;
    private final Runnable progress = this::progress;
    private boolean framesWanted;
    private final Choreographer.FrameCallback framePulse = this::frame;

    PixelPlayback(Activity activity, PixelVideoComponent owner) {
        this.activity = activity;
        this.owner = owner;
        output = new PixelOutput(this);
        seeks = new PixelSeeks(this);
        audio = new PixelAudio(this);
    }
    interface Operation { void run() throws Exception; }
    void post(Operation operation) {
        handler.post(() -> {
            if (released) return;
            try { operation.run(); }
            catch (Exception error) { error("decoderFailed", "Native video operation failed (" + error.getClass().getSimpleName() + ")"); }
        });
    }
    boolean awaitMain(Operation operation) {
        if (released) return true;
        AtomicBoolean success = new AtomicBoolean(false);
        CountDownLatch completion = new CountDownLatch(1);
        Runnable job = () -> {
            try { if (!released) operation.run(); success.set(true); }
            catch (Exception error) { android.util.Log.w("HongguoVideo", "Native operation rejected: " + error.getClass().getSimpleName()); }
            finally { completion.countDown(); }
        };
        if (Looper.myLooper() == Looper.getMainLooper()) job.run(); else handler.post(job);
        try { return completion.await(3, TimeUnit.SECONDS) && success.get(); }
        catch (InterruptedException error) { Thread.currentThread().interrupt(); return false; }
    }
    void apply(JSONObject next, long version) throws Exception {
        JSONObject source = next.optJSONObject("source");
        String id = source == null ? "" : source.getString("id");
        if (version != generation) {
            releaseSource();
            generation = version;
            sourceId = id;
            output.sourceChanged();
        }
        props = next;
        output.root.setAlpha(next.getBoolean("active") ? 1f : 0f);
        if (source == null) return;
        if (direct == null) {
            String frames = source.optString("frameSource", "");
            if (frames.isEmpty()) throw new java.io.IOException("Android playback requires a frame source");
            direct = new PixelDirect(this, version, frames);
            output.prepare();
            prepared = true;
            emit("loadStart");
            direct.prepared();
        }
        controls(true);
    }
    private void controls(boolean claimOwner) throws Exception {
        handler.removeCallbacks(progress);
        if (!prepared || direct == null || props == null) return;
        if (!claimOwner && !direct.isOwner()) { audio.abandon(); return; }
        boolean wanted = props.getBoolean("active") && !props.getBoolean("paused") && direct.mayPlay();
        if (!wanted) audio.update(false);
        else if (!audio.interrupted) audio.update(true);
        boolean playing = wanted && !audio.interrupted && audio.granted();
        direct.control(playing, (float) props.getDouble("volume"), props.getBoolean("muted"), props.getDouble("rate"), claimOwner);
        updateFrames(playing);
        if (framesWanted) handler.postDelayed(progress, interval());
    }
    private int interval() { return props == null ? 250 : Math.max(16, Math.min(10_000, props.optInt("progressIntervalMs", 250))); }
    void reapplyControls() {
        try {
            String latest = owner.latestState(generation);
            if (latest == null) return;
            props = new JSONObject(latest);
            controls(false);
        } catch (Exception error) { error("decoderFailed", "Native PCM controls could not be restored"); }
    }
    void interruptAudio() {
        if (direct == null || props == null) return;
        try { direct.control(false, (float) props.getDouble("volume"), props.getBoolean("muted"), props.getDouble("rate"), false); }
        catch (Exception error) { error("decoderFailed", "Native PCM interruption failed"); }
        updateFrames(false);
    }
    private void updateFrames(boolean playing) {
        boolean wanted = active() && direct != null && (playing || direct.needsFrames());
        if (wanted == framesWanted) return;
        framesWanted = wanted;
        if (wanted) Choreographer.getInstance().postFrameCallback(framePulse);
        else Choreographer.getInstance().removeFrameCallback(framePulse);
    }
    private void frame(long ignoredFrameTimeNanos) {
        if (!framesWanted || released || direct == null) return;
        try {
            if (!direct.isOwner()) { stopFrames(); return; }
            output.frame();
            owner.frameTick();
            Choreographer.getInstance().postFrameCallback(framePulse);
        } catch (Exception error) { stopFrames(); error("decoderFailed", "Display scheduling failed"); }
    }
    private void stopFrames() {
        framesWanted = false;
        Choreographer.getInstance().removeFrameCallback(framePulse);
    }
    private void progress() {
        if (released || !prepared || direct == null || props == null || !active()) return;
        try {
            JSONObject status = direct.status();
            emit("progress", "positionMs", status.getLong("positionMs"), "durationMs", status.opt("durationMs"));
            direct.audioBuffering(status.getBoolean("isBuffering"));
            if (status.getBoolean("isEnded")) { audio.abandon(); direct.ended(); updateFrames(false); }
        } catch (Exception error) { error("decoderFailed", "Native PCM position is unavailable"); return; }
        if (framesWanted) handler.postDelayed(progress, interval());
    }
    void emit(String type, Object... fields) { owner.event(PixelJson.event(type, sourceId, fields)); }
    Object duration() {
        try { return direct == null ? JSONObject.NULL : direct.status().opt("durationMs"); }
        catch (Exception error) { error("decoderFailed", "Native media duration is unavailable"); return JSONObject.NULL; }
    }
    boolean active() { return props != null && props.optBoolean("active"); }
    void error(String code, String detail) { if (!released) emit("error", "code", code, "detail", detail); }
    private void releaseSource() {
        stopFrames();
        handler.removeCallbacks(progress);
        seeks.clear();
        prepared = false;
        audio.abandon();
        if (direct != null) { direct.close(); direct = null; }
    }
    void release() {
        if (released) return;
        released = true;
        releaseSource();
        audio.release();
        output.release();
        handler.removeCallbacksAndMessages(null);
    }
}
