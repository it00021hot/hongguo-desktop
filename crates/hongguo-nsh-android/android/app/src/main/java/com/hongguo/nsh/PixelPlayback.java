package com.hongguo.nsh;

import android.app.Activity;
import android.media.AudioAttributes;
import android.media.MediaPlayer;
import android.media.PlaybackParams;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import org.json.JSONException;
import org.json.JSONObject;

/** One platform decoder/clock. Every MediaPlayer operation runs on this main handler. */
final class PixelPlayback {
    final Activity activity;
    final PixelVideoComponent owner;
    final Handler handler = new Handler(Looper.getMainLooper());
    final PixelOutput output;
    final PixelSeeks seeks;
    final PixelAudio audio;
    MediaPlayer player;
    private PixelDataSource dataSource;
    long generation;
    String sourceId = "";
    boolean prepared;
    private boolean reportedProgress;
    private boolean reportedFrame;
    private JSONObject props;
    private volatile boolean released;
    private final Runnable progress = this::progress;

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
        String url = source == null ? "" : source.getString("url");
        boolean changed = version != generation;
        if (changed) {
            releasePlayer();
            generation = version;
            sourceId = id;
            reportedProgress = false;
            reportedFrame = false;
            output.sourceChanged();
        }
        props = next;
        output.root.setAlpha(next.getBoolean("active") ? 1f : 0f);
        output.resizeMode = next.getString("resizeMode");
        output.fit();
        if (source == null || url.isEmpty()) return;
        if (player == null && changed) create(url, version);
        controls();
    }
    private void create(String url, long version) throws Exception {
        final MediaPlayer candidate = new MediaPlayer();
        player = candidate;
        candidate.setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MOVIE).build());
        candidate.setSurface(output.rawSurface);
        candidate.setOnPreparedListener(ready -> {
            if (!current(ready, version)) return;
            prepared = true;
            output.videoWidth = ready.getVideoWidth();
            output.videoHeight = ready.getVideoHeight();
            output.fit();
            emit("load", "durationMs", duration(ready), "width", output.videoWidth, "height", output.videoHeight);
            seeks.prepared();
            reapplyControls();
        });
        candidate.setOnVideoSizeChangedListener((video, width, height) -> {
            if (!current(video, version)) return;
            output.videoWidth = width; output.videoHeight = height; output.fit();
            if (prepared) emit("load", "durationMs", duration(video), "width", width, "height", height);
        });
        candidate.setOnCompletionListener(video -> { if (current(video, version)) { audio.abandon(); emit("end"); } });
        candidate.setOnSeekCompleteListener(video -> { if (current(video, version)) seeks.complete(); });
        candidate.setOnInfoListener((video, what, extra) -> {
            if (!current(video, version)) return true;
            if (what == MediaPlayer.MEDIA_INFO_VIDEO_RENDERING_START && !reportedFrame) {
                reportedFrame = true;
                android.util.Log.i("HongguoVideo", "First native frame generation=" + version);
            }
            if (what == MediaPlayer.MEDIA_INFO_BUFFERING_START) emit("buffer", "isBuffering", true);
            if (what == MediaPlayer.MEDIA_INFO_BUFFERING_END || what == MediaPlayer.MEDIA_INFO_VIDEO_RENDERING_START) emit("buffer", "isBuffering", false);
            return false;
        });
        candidate.setOnErrorListener((video, what, extra) -> {
            if (current(video, version)) { audio.abandon(); error("decoderFailed", "MediaPlayer error " + what + "/" + extra); }
            return true;
        });
        emit("loadStart");
        dataSource = owner.openDataSource(version, url);
        if (dataSource != null) candidate.setDataSource(dataSource);
        else candidate.setDataSource(activity, Uri.parse(url));
        candidate.prepareAsync();
    }
    private boolean current(MediaPlayer video, long version) { return !released && video == player && generation == version; }
    private void controls() throws JSONException {
        handler.removeCallbacks(progress);
        if (player == null || !prepared || props == null) return;
        boolean play = props.getBoolean("active") && !props.getBoolean("paused");
        float volume = props.getBoolean("muted") ? 0f : (float) props.getDouble("volume");
        player.setVolume(volume, volume);
        if (!play) {
            if (player.isPlaying()) player.pause();
            audio.update(false);
        } else if (!audio.interrupted) {
            if (!player.isPlaying()) player.start();
            // setPlaybackParams starts a paused player; defer speed until playback.
            float speed = (float) props.getDouble("rate");
            if (Math.abs(player.getPlaybackParams().getSpeed() - speed) > 0.0001f) player.setPlaybackParams(new PlaybackParams().allowDefaults().setSpeed(speed));
            audio.update(true);
        }
        if (props.getBoolean("active")) handler.postDelayed(progress, interval());
    }
    private int interval() { return props == null ? 250 : Math.max(16, Math.min(10_000, props.optInt("progressIntervalMs", 250))); }
    void reapplyControls() {
        try {
            String latest=owner.latestState(generation);
            if (latest == null) return; // A newer source or disposal owns subsequent controls.
            props=new JSONObject(latest);
            controls();
        } catch (Exception error) { error("decoderFailed", "MediaPlayer could not restore playback controls"); }
    }
    private void progress() {
        if (released || player == null || !prepared || props == null || !props.optBoolean("active")) return;
        try {
            int position = Math.max(0, player.getCurrentPosition());
            if (!reportedProgress && position > 0) {
                reportedProgress = true;
                android.util.Log.i("HongguoVideo", "First native progress generation=" + generation + " positionMs=" + position);
            }
            emit("progress", "positionMs", position, "durationMs", duration(player));
        }
        catch (RuntimeException error) { error("decoderFailed", "MediaPlayer position is unavailable"); return; }
        handler.postDelayed(progress, interval());
    }
    void emit(String type, Object... fields) { owner.event(PixelJson.event(type, sourceId, fields)); }
    private static Object duration(MediaPlayer video) { int value=video.getDuration(); return value>0 ? value : JSONObject.NULL; }
    void error(String code, String detail) { if (!released) emit("error", "code", code, "detail", detail); }
    private void releasePlayer() {
        handler.removeCallbacks(progress);
        seeks.clear();
        prepared = false;
        audio.abandon();
        MediaPlayer previous = player;
        player = null;
        // A platform release can wait for an in-flight readAt. Cancel it first.
        if (dataSource != null) { dataSource.close(); dataSource = null; }
        if (previous != null) previous.release();
    }
    void release() {
        if (released) return;
        released = true;
        releasePlayer();
        output.release();
        handler.removeCallbacksAndMessages(null);
    }
}
