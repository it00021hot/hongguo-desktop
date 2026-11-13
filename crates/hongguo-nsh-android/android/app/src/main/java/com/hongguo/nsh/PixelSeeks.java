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
    private boolean audioCompleted;
    private final Runnable deadline = this::timeout;
    PixelSeeks(PixelPlayback playback) { this.playback = playback; }
    boolean pending() { return inFlight != null || queued != null; }
    boolean audioPending() { return inFlight != null && !audioCompleted; }
    void request(JSONObject request) throws JSONException {
        if (!request.getString("sourceId").equals(playback.sourceId)) return;
        if (blocked) { playback.error("seekFailed", "A previous platform seek did not complete"); return; }
        queued = request;
        advance();
    }
    private void advance() {
        if (blocked || inFlight != null || !playback.prepared || playback.player == null) return;
        if (queued == null) return;
        try {
            inFlight = queued;
            queued = null;
            audioCompleted = false;
            playback.direct.beginSeek();
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
        try { playback.direct.audioSeekCompleted(Math.max(0, playback.player.getCurrentPosition())); audioCompleted = true; }
        catch (java.io.IOException | RuntimeException error) { timeout(); }
    }
    void videoComplete() {
        if (inFlight != null && audioCompleted) finish();
    }
    private void finish() {
        playback.handler.removeCallbacks(deadline);
        try {
            playback.emit("seeked", "requestId", inFlight.getString("requestId"), "positionMs", Math.max(0, playback.player.getCurrentPosition()));
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
        playback.error("seekFailed", detail);
    }
    void clear() {
        playback.handler.removeCallbacks(deadline);
        inFlight = null;
        queued = null;
        blocked = false;
        audioCompleted = false;
    }
}
