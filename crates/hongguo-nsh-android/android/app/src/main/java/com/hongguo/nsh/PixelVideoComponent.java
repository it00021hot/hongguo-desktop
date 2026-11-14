package com.hongguo.nsh;

import android.app.Activity;
import android.view.Surface;
import android.view.View;
import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import java.util.Map;
import org.json.JSONObject;

/** System ABI only. Rust owns typed props, source epochs, rendering and enhancement state. */
public final class PixelVideoComponent implements NshComponent {
    private final NativeEventSink events;
    final PixelPlayback playback;
    private volatile long handle;
    private volatile boolean closed;

    public PixelVideoComponent(Activity activity, NshComponentContext context) {
        this(activity, context::emit);
    }
    public PixelVideoComponent(Activity activity, NativeEventSink events) {
        this.events = events;
        playback = new PixelPlayback(activity, this);
        handle = nativeCreate();
    }
    @Override public View view() { return playback.output.root; }
    @Override public void setProp(String name, Object value) {
        if (closed || !name.equals("state") || !(value instanceof Map<?, ?> map)) return;
        nativeSetState(handle, new JSONObject(map).toString());
    }
    @Override public void command(String name, Object value) {
        if (closed || !name.equals("seek") || !(value instanceof Map<?, ?> map)) return;
        nativeSeek(handle, new JSONObject(map).toString());
    }
    public boolean applyState(String json, long generation) {
        return playback.awaitMain(() -> {
            String latest=latestState(generation);
            if (latest != null) playback.apply(new JSONObject(latest), generation);
        });
    }
    public void requestSeek(String json, long generation) {
        playback.post(() -> {
            if (!closed && playback.generation == generation) playback.seeks.request(new JSONObject(json));
        });
    }
    public void prepareFrameOutput(long generation) {
        playback.post(() -> { if (!closed && playback.generation == generation) playback.output.prepare(); });
    }
    public void showFrames(long generation, boolean visible) {
        playback.post(() -> {
            if (!closed && playback.generation == generation) playback.output.show(visible);
        });
    }
    public void emitVideo(String json, long ignored) {
        playback.post(() -> {
            if (!closed) events.emit("video", PixelJson.map(new JSONObject(json)));
        });
    }
    void event(JSONObject event) {
        if (!closed && handle != 0) nativeEvent(handle, playback.generation, event.toString());
    }
    String latestState(long generation) {
        return closed || handle == 0 ? null : nativeLatestState(handle, generation);
    }
    long nativeHandle() throws java.io.IOException {
        if (closed || handle == 0) throw new java.io.IOException("Media controller is closed");
        return handle;
    }
    public void directPresented(long epoch, long generation, long serial, long pts, long token, long outputVersion, int width, int height, boolean swapSucceeded) {
        playback.post(() -> {
            if (!closed && playback.generation == epoch && playback.direct != null)
                playback.output.submitted(outputVersion, token, swapSucceeded, () -> {
                    if (!closed && playback.generation == epoch && playback.direct != null)
                        playback.direct.presented(generation, serial, pts, width, height);
                });
        });
    }
    public void useSdrOutput(long epoch) {
        playback.post(() -> { if (!closed && playback.generation == epoch) playback.output.useSdr(); });
    }
    public void sourceHdr(long epoch, boolean hdr) {
        playback.post(() -> { if (!closed && playback.generation == epoch) playback.output.sourceHdr(hdr); });
    }
    public void directHolding(long epoch, boolean holding) {
        playback.post(() -> {
            if (!closed && playback.generation == epoch && playback.direct != null) playback.direct.holding(holding);
        });
    }
    public void directGap(long epoch, int width, int height) {
        playback.post(() -> {
            if (!closed && playback.generation == epoch && playback.direct != null) {
                playback.output.videoWidth = width; playback.output.videoHeight = height;
                playback.direct.emptyVideo();
            }
        });
    }
    /** Render-worker call: SurfaceTexture must attach to that worker's current EGL context. */
    public PixelDecodeSurface createDecodeSurface(int texture, int width, int height, boolean p010) throws java.io.IOException {
        return new PixelDecodeSurface(texture, width, height, p010, this::frameTick);
    }
    long viewport(Surface surface, int width, int height) {
        return viewport(surface, width, height, 0f, false);
    }
    long viewport(Surface surface, int width, int height, float hdrPeak, boolean floatingRequested) {
        return !closed && handle != 0 ? nativeViewport(handle, surface, width, height, hdrPeak, floatingRequested) : -1;
    }
    void frameTick() { if (!closed && handle != 0) nativeFrameTick(handle); }
    @Override public void destroy() {
        if (closed) return;
        playback.release();
        closed = true;
        long previous = handle;
        handle = 0;
        nativeDestroy(previous);
    }
    private native long nativeCreate();
    private static native void nativeSetState(long handle, String state);
    private static native String nativeLatestState(long handle, long generation);
    private static native void nativeSeek(long handle, String request);
    private static native long nativeViewport(long handle, Surface surface, int width, int height, float hdrPeak, boolean floatingRequested);
    private static native void nativeEvent(long handle, long generation, String event);
    private static native void nativeDestroy(long handle);
    private static native void nativeFrameTick(long handle);
}
