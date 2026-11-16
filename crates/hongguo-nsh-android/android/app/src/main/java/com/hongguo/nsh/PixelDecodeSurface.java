package com.hongguo.nsh;

import android.graphics.ImageFormat;
import android.graphics.SurfaceTexture;
import android.hardware.HardwareBuffer;
import android.media.Image;
import android.media.ImageReader;
import android.media.MediaCodecInfo;
import android.media.MediaCodecList;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.view.Surface;
import java.util.concurrent.atomic.AtomicBoolean;

/** Offscreen decoder sink. Its images never pass through an Android RGB compositor. */
public final class PixelDecodeSurface {
    private final SurfaceTexture texture;
    private final ImageReader reader;
    private final Surface output;
    private final AtomicBoolean pending = new AtomicBoolean();

    public PixelDecodeSurface(int name, int width, int height, boolean p010, Runnable available) throws java.io.IOException {
        if (Build.VERSION.SDK_INT < 29 || !hasHardwareDecoder(width, height, p010))
            throw new java.io.IOException("No proven hardware video decoder for this output");
        if (p010) {
            if (Build.VERSION.SDK_INT < 33) throw new java.io.IOException("P010 requires Android 13");
            reader = ImageReader.newInstance(width, height, ImageFormat.YCBCR_P010, 3,
                HardwareBuffer.USAGE_GPU_SAMPLED_IMAGE);
            reader.setOnImageAvailableListener(ignored -> available.run(), new Handler(Looper.getMainLooper()));
            texture = null;
            output = reader.getSurface();
        } else {
            reader = null;
            texture = new SurfaceTexture(name);
            texture.setDefaultBufferSize(width, height);
            texture.setOnFrameAvailableListener(ignored -> { pending.set(true); available.run(); }, new Handler(Looper.getMainLooper()));
            output = new Surface(texture);
        }
    }

    private static boolean hasHardwareDecoder(int width, int height, boolean main10) {
        for (MediaCodecInfo codec : new MediaCodecList(MediaCodecList.REGULAR_CODECS).getCodecInfos()) {
            if (codec.isEncoder() || !codec.isHardwareAccelerated() || codec.isSoftwareOnly()) continue;
            for (String type : codec.getSupportedTypes()) {
                if (!type.equalsIgnoreCase("video/hevc") && (!type.equalsIgnoreCase("video/avc") || main10)) continue;
                try {
                    MediaCodecInfo.CodecCapabilities caps = codec.getCapabilitiesForType(type);
                    if (!caps.getVideoCapabilities().isSizeSupported(width, height)) continue;
                    if (!main10) return true;
                    for (MediaCodecInfo.CodecProfileLevel profile : caps.profileLevels) {
                        if (profile.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10
                            || profile.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10
                            || profile.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10Plus) return true;
                    }
                } catch (IllegalArgumentException ignored) { /* This candidate cannot serve this stream. */ }
            }
        }
        return false;
    }

    public Surface surface() { return output; }
    public long nextTimestamp() { return System.nanoTime(); }
    /** Called with the texture's owning EGL context current. Zero means no new buffer. */
    public long update(float[] transform) {
        if (texture == null || !pending.getAndSet(false)) return 0;
        texture.updateTexImage();
        texture.getTransformMatrix(transform);
        return texture.getTimestamp();
    }
    public Image acquireImage() { return reader == null ? null : reader.acquireNextImage(); }
    public void release() {
        output.release();
        if (texture != null) texture.release();
        if (reader != null) reader.close();
    }
}
