package com.hongguo.nsh;

import android.media.MediaPlayer;
import org.json.JSONException;
import org.json.JSONObject;

/** Serializes platform seeks: Android's callback has no request identifier. */
final class PixelSeeks {
    private final PixelPlayback playback;
    private JSONObject inFlight;
    private JSONObject queued;
    private boolean blocked;
    private boolean redrawRequested;
    private boolean redrawInFlight;
    private final Runnable deadline = this::timeout;
    PixelSeeks(PixelPlayback playback) { this.playback = playback; }
    void request(JSONObject request) throws JSONException {
        if (!request.getString("sourceId").equals(playback.sourceId)) return;
        if (blocked) { playback.error("seekFailed", "A previous platform seek did not complete"); return; }
        queued = request;
        advance();
    }
    void redraw() { redrawRequested = true; advance(); }
    private void advance() {
        if (blocked || inFlight != null || !playback.prepared || playback.player == null) return;
        if (queued == null && !redrawRequested) return;
        try {
            redrawInFlight = queued == null;
            if (redrawInFlight) {
                redrawRequested = false;
                inFlight = new JSONObject().put("positionMs", playback.player.getCurrentPosition());
            } else {
                inFlight = queued;
                queued = null;
                // The user seek itself supplies the current image to the new Surface.
                redrawRequested = false;
            }
            playback.player.seekTo(inFlight.getLong("positionMs"), MediaPlayer.SEEK_CLOSEST);
            playback.handler.postDelayed(deadline, 15_000);
        } catch (RuntimeException | JSONException error) {
            failure("MediaPlayer rejected the seek");
            blocked = true;
            inFlight = null;
            queued = null;
        }
    }
    void prepared() { advance(); }
    void complete() {
        if (blocked && inFlight == null) { blocked = false; return; }
        if (inFlight == null || playback.player == null || !playback.prepared) return;
        playback.handler.removeCallbacks(deadline);
        try {
            if (!redrawInFlight) playback.emit("seeked", "requestId", inFlight.getString("requestId"), "positionMs", Math.max(0, playback.player.getCurrentPosition()));
        } catch (JSONException | RuntimeException error) { failure("MediaPlayer could not confirm the seek position"); }
        inFlight = null;
        advance();
        // PlaybackCompleted does not restart after seek by itself. Reapply the
        // latest intent only after the final queued seek actually completes.
        if (inFlight == null) playback.reapplyControls();
    }
    private void timeout() {
        if (inFlight == null) return;
        // A late anonymous callback must never acknowledge a newer request.
        queued = null;
        inFlight = null;
        blocked = true;
        failure("MediaPlayer seek completion timed out");
    }
    private void failure(String detail) {
        if (redrawInFlight) android.util.Log.w("HongguoVideo", "Native frame redraw did not complete: " + detail);
        else playback.error("seekFailed", detail);
    }
    void clear() {
        playback.handler.removeCallbacks(deadline);
        inFlight = null;
        queued = null;
        blocked = false;
        redrawRequested = false;
        redrawInFlight = false;
    }
}
