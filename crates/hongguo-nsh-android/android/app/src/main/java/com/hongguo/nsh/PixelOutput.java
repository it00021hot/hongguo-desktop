package com.hongguo.nsh;

import android.graphics.Color;
import android.graphics.SurfaceTexture;
import android.graphics.PixelFormat;
import android.graphics.drawable.ColorDrawable;
import android.view.Display;
import android.view.Surface;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.TextureView;
import android.view.View;
import android.widget.FrameLayout;
import com.longbridge.nsh.SurfaceComposition;

/** TextureViews composite and clip within NSH's native widget, below its controls. */
final class PixelOutput {
    final FrameLayout root;
    private TextureView frames;
    private SurfaceView hdr;
    private boolean sdrOnly;
    private boolean awaitingAttach;
    private boolean awaitingLayout;
    private boolean floatingRequested;
    private boolean hdrSource;
    private boolean released;
    private Surface presentationSurface;
    private long surfaceVersion;
    private final PixelPlayback playback;
    int videoWidth;
    int videoHeight;
    private final PixelPresentationGate presentation = new PixelPresentationGate();
    private final Runnable presentationDeadline = this::presentationTimedOut;
    private void presentationTimedOut() {
        if (presentation.pending()) {
            presentation.clear();
            playback.error("gpuFailed", "The output surface did not confirm its exact presentation token");
        }
    }

    PixelOutput(PixelPlayback playback) {
        this.playback = playback;
        root = new FrameLayout(playback.activity);
        root.setBackgroundColor(Color.BLACK);
        root.setClipChildren(true);
        root.getViewTreeObserver().addOnPreDrawListener(() -> {
            reconcileComposition();
            return true;
        });
    }
    void prepare() {
        if (released) return;
        if (frames != null || hdr != null) return;
        if (!root.isAttachedToWindow()) {
            if (!awaitingAttach) {
                awaitingAttach = true;
                root.addOnAttachStateChangeListener(new View.OnAttachStateChangeListener() {
                    @Override public void onViewAttachedToWindow(View view) {
                        root.removeOnAttachStateChangeListener(this);
                        awaitingAttach = false;
                        root.post(PixelOutput.this::prepare);
                    }
                    @Override public void onViewDetachedFromWindow(View view) {}
                });
            }
            return;
        }
        if (root.getWidth() <= 0 || root.getHeight() <= 0) {
            if (!awaitingLayout) {
                awaitingLayout = true;
                root.addOnLayoutChangeListener(new View.OnLayoutChangeListener() {
                    @Override public void onLayoutChange(View view, int l, int t, int r, int b, int oldL, int oldT, int oldR, int oldB) {
                        if (r <= l || b <= t) return;
                        root.removeOnLayoutChangeListener(this);
                        awaitingLayout = false;
                        prepare();
                    }
                });
            }
            return;
        }
        floatingRequested = capabilityPeak() > 0f;
        float peak = hdrPeak();
        if (peak > 0f) { prepareHdr(peak); return; }
        prepareSdr();
    }
    private float hdrPeak() {
        return SurfaceComposition.supports(root) ? capabilityPeak() : 0f;
    }
    private float capabilityPeak() {
        if (sdrOnly) return 0f;
        Display display = root.getDisplay();
        if (display == null || !display.isWideColorGamut()) return 0f;
        Display.HdrCapabilities capabilities = display.getHdrCapabilities();
        boolean supported = false;
        for (int type : capabilities.getSupportedHdrTypes())
            supported |= type == Display.HdrCapabilities.HDR_TYPE_HDR10 || type == Display.HdrCapabilities.HDR_TYPE_HLG;
        float peak = capabilities.getDesiredMaxLuminance();
        return supported && Float.isFinite(peak) && peak >= 100f && peak <= 10_000f ? peak : 0f;
    }
    private void prepareHdr(float peak) {
        hdr = new SurfaceView(playback.activity);
        SurfaceView view = hdr;
        // Keep the Surface below the NSH window: controls and dialogs remain above it.
        hdr.getHolder().setFormat(PixelFormat.RGBA_F16);
        hdr.getHolder().addCallback(new SurfaceHolder.Callback() {
            @Override public void surfaceCreated(SurfaceHolder holder) {}
            @Override public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
                if (hdr != view) return;
                presentationSurface = holder.getSurface();
                viewport(presentationSurface, width, height, peak);
            }
            @Override public void surfaceDestroyed(SurfaceHolder holder) {
                if (hdr != view) return;
                viewport(null, 0, 0, 0f);
                presentationSurface = null; // The holder owns this Surface.
            }
        });
        root.addView(hdr, new FrameLayout.LayoutParams(-1, -1));
        show(false);
    }
    void useSdr() {
        useSdr(true);
    }
    void sourceHdr(boolean sourceHdr) {
        hdrSource = sourceHdr;
        if (!sourceHdr && hdr != null) { useSdr(false); return; }
        float peak = hdrPeak();
        if (!sourceHdr || hdr != null || peak <= 0f) return;
        if (frames != null) {
            frames.setSurfaceTextureListener(null);
            viewport(null, 0, 0, 0f);
            if (presentationSurface != null) presentationSurface.release();
            presentationSurface = null;
            TextureView previous = frames;
            frames = null;
            root.removeView(previous);
        }
        prepareHdr(peak);
    }
    private void useSdr(boolean permanent) {
        if (sdrOnly && frames != null) return;
        sdrOnly |= permanent;
        if (permanent) floatingRequested = false;
        presentation.clear();
        playback.handler.removeCallbacks(presentationDeadline);
        if (hdr != null) {
            SurfaceView previous = hdr;
            hdr = null;
            viewport(null, 0, 0, 0f);
            root.removeView(previous);
            presentationSurface = null;
        }
        root.setForeground(null);
        prepareSdr();
        if (permanent && presentationSurface != null && frames != null)
            viewport(presentationSurface, frames.getWidth(), frames.getHeight(), 0f);
    }
    private void prepareSdr() {
        if (frames != null) return;
        frames = new TextureView(playback.activity);
        TextureView view = frames;
        frames.setOpaque(false);
        frames.setAlpha(0f);
        frames.setSurfaceTextureListener(new TextureView.SurfaceTextureListener() {
            @Override public void onSurfaceTextureAvailable(SurfaceTexture texture, int width, int height) {
                if (frames != view) return;
                presentationSurface = new Surface(texture);
                viewport(presentationSurface, width, height, 0f);
            }
            @Override public void onSurfaceTextureSizeChanged(SurfaceTexture texture, int width, int height) {
                if (frames != view) return;
                viewport(presentationSurface, width, height, 0f);
            }
            @Override public boolean onSurfaceTextureDestroyed(SurfaceTexture texture) {
                if (frames != view) return true;
                viewport(null, 0, 0, 0f);
                if (presentationSurface != null) presentationSurface.release();
                presentationSurface = null;
                return true;
            }
            @Override public void onSurfaceTextureUpdated(SurfaceTexture texture) { if (frames == view) acknowledge(texture); }
        });
        root.addView(frames, new FrameLayout.LayoutParams(-1, -1));
    }
    void show(boolean visible) {
        if (frames != null) frames.setAlpha(visible ? 1f : 0f);
        if (hdr != null) root.setForeground(visible ? null : new ColorDrawable(Color.BLACK));
    }
    private void viewport(Surface surface, int width, int height, float peak) {
        surfaceVersion = playback.owner.viewport(surface, width, height, peak, floatingRequested);
        presentation.surfaceChanged(surfaceVersion);
    }
    void submitted(long outputVersion, long token, boolean swapSucceeded, Runnable presented) {
        if (outputVersion != surfaceVersion) return;
        playback.handler.removeCallbacks(presentationDeadline);
        if (!presentation.submittedOnSurface(playback.generation, outputVersion, token, presented)) return;
        if (hdr != null && presentationSurface != null && presentationSurface.isValid()
                && playback.active() && presentation.windowSubmitted(playback.generation, token, swapSucceeded)) return;
        SurfaceTexture texture = frames == null ? null : frames.getSurfaceTexture();
        if (texture != null) acknowledge(texture);
        if (presentation.pending()) playback.handler.postDelayed(presentationDeadline, 3_000);
    }
    private void acknowledge(SurfaceTexture texture) {
        if (presentation.updated(playback.generation, texture.getTimestamp()))
            playback.handler.removeCallbacks(presentationDeadline);
    }
    void frame() {
        reconcileComposition();
        // onSurfaceTextureUpdated is dispatched before RenderThread dequeue on
        // some Android releases. Read the exact token again on display pulses.
        if (presentation.pending() && frames != null && frames.getSurfaceTexture() != null)
            acknowledge(frames.getSurfaceTexture());
    }
    private void reconcileComposition() {
        if (released || !playback.prepared) return;
        if (hdr != null && !SurfaceComposition.supports(root)) { useSdr(false); return; }
        if (hdr == null && frames != null && hdrSource && !sdrOnly && hdrPeak() > 0f) sourceHdr(true);
    }
    void sourceChanged() {
        playback.handler.removeCallbacks(presentationDeadline);
        presentation.clear();
        videoWidth = 0;
        videoHeight = 0;
        show(false);
        sdrOnly = false;
        hdrSource = false;
        floatingRequested = false;
        if (frames != null) frames.setSurfaceTextureListener(null);
        if (presentationSurface != null && hdr == null) presentationSurface.release();
        presentationSurface = null;
        TextureView oldFrames = frames;
        SurfaceView oldHdr = hdr;
        frames = null;
        hdr = null;
        viewport(null, 0, 0, 0f);
        if (oldFrames != null) root.removeView(oldFrames);
        if (oldHdr != null) root.removeView(oldHdr);
        root.setForeground(null);
    }
    void release() {
        released = true;
        sourceChanged();
    }
}
