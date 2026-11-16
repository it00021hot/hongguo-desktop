package com.hongguo.nsh;

/** Pure generation gate: audio cannot outrun the frame actually submitted to the GPU. */
final class PixelDirectGate {
    private long generation = -1;
    private boolean ready;
    private boolean holding = true;
    private boolean audioSeeking;
    private boolean audioBuffering;
    private boolean closed;

    boolean mayPlay() { return !closed && ready && !holding; }
    boolean ready() { return !closed && ready; }
    boolean holding() { return holding; }
    boolean buffering() { return holding || audioBuffering; }
    void audioBuffering(boolean buffering) { audioBuffering = buffering; }
    void beginSeek() { ready = false; holding = true; audioSeeking = true; }
    void seekCompleted(long generation) { this.generation = generation; ready = false; holding = true; audioSeeking = false; }
    boolean accepts(long generation) { return !closed && !audioSeeking && generation >= this.generation; }
    void presented(long generation) { this.generation = generation; ready = true; holding = false; }
    void emptyVideo() { if (!closed && !audioSeeking) { ready = true; holding = false; } }
    void holding(boolean holding) { this.holding = holding; }
    void close() { closed = true; ready = false; holding = true; }
}
