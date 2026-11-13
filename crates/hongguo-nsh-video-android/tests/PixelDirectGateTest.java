package com.hongguo.nsh;

public final class PixelDirectGateTest {
    private static void require(boolean value) { if (!value) throw new AssertionError(); }
    public static void main(String[] ignored) {
        PixelDirectGate gate = new PixelDirectGate();
        require(!gate.mayPlay());
        require(gate.buffering());
        gate.audioBuffering(false);
        require(gate.buffering());
        require(gate.accepts(0));
        gate.presented(0);
        require(gate.mayPlay());
        require(!gate.buffering());
        gate.audioBuffering(true);
        require(gate.buffering());
        gate.audioBuffering(false);
        require(!gate.buffering());
        gate.holding(true);
        require(!gate.mayPlay());
        gate.holding(false);
        require(gate.mayPlay());
        gate.beginSeek();
        require(!gate.mayPlay());
        require(!gate.accepts(0));
        gate.seekCompleted(1);
        require(!gate.accepts(0));
        require(gate.accepts(1));
        require(!gate.mayPlay());
        gate.presented(1);
        require(gate.mayPlay());
        gate.seekCompleted(2);
        require(!gate.accepts(1));
        gate.presented(2);
        require(gate.mayPlay());
        gate.close();
        require(!gate.accepts(2));
        require(!gate.mayPlay());
        require(!gate.ready());
        System.out.println("PASS first frame / hold / bidirectional seek generation / close");
    }
}
