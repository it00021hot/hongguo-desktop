package com.hongguo.nsh;

/** Pure one-shot gate for the exact buffer consumed by TextureView. Main thread only. */
final class PixelPresentationGate {
    private long epoch;
    private long token;
    private Runnable presented;
    private long surfaceVersion;
    boolean pending() { return presented != null; }
    void submitted(long epoch, long token, Runnable presented) {
        this.epoch = epoch;
        this.token = token;
        this.presented = presented;
    }
    void surfaceChanged(long version) {
        if (surfaceVersion != version) { surfaceVersion = version; clear(); }
    }
    boolean submittedOnSurface(long epoch, long version, long token, Runnable presented) {
        if (version < 0 || version != surfaceVersion) return false;
        submitted(epoch, token, presented);
        return true;
    }
    boolean updated(long epoch, long token) {
        if (presented == null || this.epoch != epoch || this.token != token) return false;
        Runnable callback = presented;
        clear();
        callback.run();
        return true;
    }
    boolean windowSubmitted(long epoch, long token, boolean eglSwapSucceeded) {
        return eglSwapSucceeded && updated(epoch, token);
    }
    void clear() { presented = null; }
}
