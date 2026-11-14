package com.hongguo.nsh;

public final class PixelPresentationGateTest {
    private static void require(boolean value) { if (!value) throw new AssertionError(); }
    public static void main(String[] ignored) {
        PixelPresentationGate gate = new PixelPresentationGate();
        int[] presented = {0};
        gate.submitted(1, 100, () -> presented[0]++);
        require(presented[0] == 0); // GPU submission/completion cannot release audio.
        require(!gate.updated(1, 99));
        require(!gate.updated(0, 100));
        gate.submitted(2, 200, () -> presented[0] += 10);
        require(!gate.updated(1, 100));
        require(!gate.updated(2, 100));
        require(gate.updated(2, 200));
        require(presented[0] == 10);
        require(!gate.updated(2, 200));
        gate.submitted(3, 300, () -> presented[0]++);
        gate.clear();
        require(!gate.updated(3, 300));
        require(!gate.pending());
        gate.submitted(4, 400, () -> presented[0]++);
        require(!gate.windowSubmitted(3, 400, true));
        require(!gate.windowSubmitted(4, 399, true));
        require(!gate.windowSubmitted(4, 400, false));
        require(gate.windowSubmitted(4, 400, true));
        require(presented[0] == 11);
        gate.surfaceChanged(7);
        require(!gate.submittedOnSurface(4, 6, 500, () -> presented[0]++));
        require(!gate.pending());
        require(gate.submittedOnSurface(4, 7, 500, () -> presented[0]++));
        gate.surfaceChanged(8); // UI replaces its surface before an old completion executes.
        require(!gate.windowSubmitted(4, 500, true));
        require(!gate.submittedOnSurface(4, 7, 501, () -> presented[0]++));
        require(gate.submittedOnSurface(4, 8, 502, () -> presented[0]++));
        require(gate.windowSubmitted(4, 502, true));
        require(presented[0] == 12);
        System.out.println("PASS exact EGL token / source epoch / compositor callback / one-shot / retirement");
    }
}
