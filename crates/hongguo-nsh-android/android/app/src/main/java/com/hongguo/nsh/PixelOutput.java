package com.hongguo.nsh;

import android.graphics.Color;
import android.graphics.Matrix;
import android.graphics.SurfaceTexture;
import android.view.Surface;
import android.view.TextureView;
import android.widget.FrameLayout;

/** TextureViews composite and clip within NSH's native widget, below its controls. */
final class PixelOutput {
    final FrameLayout root;
    final TextureView raw;
    private TextureView enhanced;
    private Surface presentationSurface;
    private SurfaceTexture inputTexture;
    private long inputToken;
    private final PixelPlayback playback;
    Surface rawSurface;
    int videoWidth;
    int videoHeight;
    String resizeMode = "contain";

    PixelOutput(PixelPlayback playback) {
        this.playback = playback;
        root = new FrameLayout(playback.activity);
        root.setBackgroundColor(Color.BLACK);
        root.setClipChildren(true);
        raw = new TextureView(playback.activity);
        raw.setOpaque(false);
        root.addView(raw, new FrameLayout.LayoutParams(-1, -1));
        raw.setSurfaceTextureListener(new TextureView.SurfaceTextureListener() {
            @Override public void onSurfaceTextureAvailable(SurfaceTexture texture, int width, int height) {
                rawSurface = new Surface(texture);
                try { if (!playback.directMode && playback.player != null && inputTexture == null) playback.player.setSurface(rawSurface); }
                catch (RuntimeException error) { playback.error("decoderFailed", "MediaPlayer display attachment failed"); }
                fit();
            }
            @Override public void onSurfaceTextureSizeChanged(SurfaceTexture texture, int width, int height) { fit(); }
            @Override public boolean onSurfaceTextureDestroyed(SurfaceTexture texture) {
                try { if (!playback.directMode && playback.player != null && inputTexture == null) playback.player.setSurface(null); }
                catch (RuntimeException error) { playback.error("decoderFailed", "MediaPlayer display detachment failed"); }
                if (rawSurface != null) rawSurface.release();
                rawSurface = null;
                return true;
            }
            @Override public void onSurfaceTextureUpdated(SurfaceTexture texture) {}
        });
    }
    void prepare() {
        if (enhanced != null) return;
        enhanced = new TextureView(playback.activity);
        enhanced.setOpaque(false);
        enhanced.setAlpha(0f);
        enhanced.setSurfaceTextureListener(new TextureView.SurfaceTextureListener() {
            @Override public void onSurfaceTextureAvailable(SurfaceTexture texture, int width, int height) {
                presentationSurface = new Surface(texture);
                playback.owner.viewport(presentationSurface, width, height);
            }
            @Override public void onSurfaceTextureSizeChanged(SurfaceTexture texture, int width, int height) {
                playback.owner.viewport(presentationSurface, width, height);
            }
            @Override public boolean onSurfaceTextureDestroyed(SurfaceTexture texture) {
                playback.owner.viewport(null, 0, 0);
                if (presentationSurface != null) presentationSurface.release();
                presentationSurface = null;
                return true;
            }
            @Override public void onSurfaceTextureUpdated(SurfaceTexture texture) {}
        });
        root.addView(enhanced, new FrameLayout.LayoutParams(-1, -1));
    }
    void bind(long generation, long token, Surface surface, SurfaceTexture texture) {
        if (playback.directMode) { playback.owner.detached(token); return; }
        if (playback.player != null) playback.player.setSurface(surface == null ? rawSurface : surface);
        if (inputTexture != null) inputTexture.setOnFrameAvailableListener(null);
        inputTexture = texture;
        if (surface == null) {
            inputToken = 0;
            show(false);
            playback.owner.detached(token);
        } else {
            inputToken = token;
            texture.setOnFrameAvailableListener(frame -> {
                if (inputTexture == frame && playback.generation == generation) playback.owner.frame(generation);
            }, playback.handler);
            if (playback.player != null && playback.prepared && !playback.player.isPlaying()) playback.seeks.redraw();
        }
    }
    void show(boolean visible) {
        if (enhanced != null) enhanced.setAlpha(visible ? 1f : 0f);
        raw.setAlpha(playback.directMode || visible ? 0f : 1f);
    }
    void sourceChanged() {
        if (inputTexture != null) inputTexture.setOnFrameAvailableListener(null);
        inputTexture = null;
        if (inputToken != 0) playback.owner.detached(inputToken);
        inputToken = 0;
        videoWidth = 0;
        videoHeight = 0;
        show(false);
    }
    void fit() {
        int width = raw.getWidth(), height = raw.getHeight();
        if (width == 0 || height == 0 || videoWidth == 0 || videoHeight == 0) return;
        Matrix transform = new Matrix();
        if (!resizeMode.equals("stretch")) {
            float sx = (float) width / videoWidth, sy = (float) height / videoHeight;
            float scale = resizeMode.equals("cover") ? Math.max(sx, sy) : Math.min(sx, sy);
            transform.setScale(videoWidth * scale / width, videoHeight * scale / height, width / 2f, height / 2f);
        }
        raw.setTransform(transform);
    }
    void release() {
        sourceChanged();
        raw.setSurfaceTextureListener(null);
        if (enhanced != null) enhanced.setSurfaceTextureListener(null);
        if (rawSurface != null) rawSurface.release();
        if (presentationSurface != null) presentationSurface.release();
        rawSurface = null;
        presentationSurface = null;
    }
}
