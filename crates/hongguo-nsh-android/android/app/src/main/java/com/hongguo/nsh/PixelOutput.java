package com.hongguo.nsh;

import android.graphics.Color;
import android.graphics.SurfaceTexture;
import android.view.Surface;
import android.view.TextureView;
import android.widget.FrameLayout;

/** TextureViews composite and clip within NSH's native widget, below its controls. */
final class PixelOutput {
    final FrameLayout root;
    private TextureView frames;
    private Surface presentationSurface;
    private final PixelPlayback playback;
    int videoWidth;
    int videoHeight;

    PixelOutput(PixelPlayback playback) {
        this.playback = playback;
        root = new FrameLayout(playback.activity);
        root.setBackgroundColor(Color.BLACK);
        root.setClipChildren(true);
    }
    void prepare() {
        if (frames != null) return;
        frames = new TextureView(playback.activity);
        frames.setOpaque(false);
        frames.setAlpha(0f);
        frames.setSurfaceTextureListener(new TextureView.SurfaceTextureListener() {
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
        root.addView(frames, new FrameLayout.LayoutParams(-1, -1));
    }
    void show(boolean visible) {
        if (frames != null) frames.setAlpha(visible ? 1f : 0f);
    }
    void sourceChanged() {
        videoWidth = 0;
        videoHeight = 0;
        show(false);
    }
    void release() {
        sourceChanged();
        if (frames != null) frames.setSurfaceTextureListener(null);
        if (presentationSurface != null) presentationSurface.release();
        presentationSurface = null;
    }
}
