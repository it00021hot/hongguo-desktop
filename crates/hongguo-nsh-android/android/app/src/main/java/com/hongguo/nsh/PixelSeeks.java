package com.hongguo.nsh;

import org.json.JSONException;
import org.json.JSONObject;

final class PixelSeeks {
    private static final long SEEK_DEADLINE_MS = 15_000;
    private final PixelPlayback playback;
    private JSONObject inFlight;
    private JSONObject queued;
    private final Runnable deadline = this::timeout;
    PixelSeeks(PixelPlayback playback) { this.playback = playback; }
    boolean pending() { return inFlight != null || queued != null; }
    void request(JSONObject request) throws JSONException {
        if (!request.getString("sourceId").equals(playback.sourceId)) return;
        queued = request;
        advance();
    }
    private void advance() {
        if (!playback.prepared || playback.direct == null || queued == null) return;
        try {
            playback.handler.removeCallbacks(deadline);
            inFlight = queued;
            queued = null;
            if (!playback.direct.seek(inFlight.getLong("positionMs"))) { inFlight = null; return; }
            playback.reapplyControls();
            playback.handler.postDelayed(deadline, SEEK_DEADLINE_MS);
        } catch (Exception error) { timeout(); }
    }
    void prepared() { advance(); }
    void videoComplete() {
        if (inFlight == null) return;
        playback.handler.removeCallbacks(deadline);
        try { playback.emit("seeked", "requestId", inFlight.getString("requestId"), "positionMs", playback.direct.status().getLong("positionMs")); }
        catch (Exception error) { playback.error("seekFailed", "Native session could not confirm seek position"); }
        inFlight = null;
        advance();
        if (inFlight == null) playback.reapplyControls();
    }
    private void timeout() {
        if (inFlight == null) return;
        queued = null;
        inFlight = null;
        playback.error("seekFailed", "Native frame presentation timed out after seek");
    }
    void clear() {
        playback.handler.removeCallbacks(deadline);
        inFlight = null;
        queued = null;
    }
}
