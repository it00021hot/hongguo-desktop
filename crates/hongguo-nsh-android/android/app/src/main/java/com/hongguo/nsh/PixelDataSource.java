package com.hongguo.nsh;

import android.media.MediaDataSource;
import java.io.IOException;
import java.util.concurrent.atomic.AtomicBoolean;

/** Thin bounded buffer bridge. Rust owns source lifetime, commit waits and EOF. */
final class PixelDataSource extends MediaDataSource {
    private final long handle;
    private final long generation;
    private final AtomicBoolean closed = new AtomicBoolean(false);

    private PixelDataSource(long handle, long generation) {
        this.handle = handle;
        this.generation = generation;
    }
    static PixelDataSource tryOpen(long handle, long generation, String url) throws IOException {
        return nativeOpen(handle, generation, url) ? new PixelDataSource(handle, generation) : null;
    }
    @Override public int readAt(long position, byte[] buffer, int offset, int size) throws IOException {
        if (position < 0 || buffer == null || offset < 0 || size < 0 || offset > buffer.length - size) {
            throw new IOException("Invalid media read buffer or range");
        }
        if (closed.get()) throw new IOException("Media reader is closed");
        if (size == 0) return 0;
        byte[] bytes = nativeRead(handle, generation, position, size);
        if (closed.get()) throw new IOException("Media reader is closed");
        if (bytes == null) return -1;
        if (bytes.length == 0 || bytes.length > size) throw new IOException("Invalid native media read length");
        System.arraycopy(bytes, 0, buffer, offset, bytes.length);
        return bytes.length;
    }
    @Override public long getSize() throws IOException {
        if (closed.get()) throw new IOException("Media reader is closed");
        return nativeSize(handle, generation);
    }
    @Override public void close() {
        if (closed.compareAndSet(false, true)) nativeClose(handle, generation);
    }
    private static native boolean nativeOpen(long handle, long generation, String url) throws IOException;
    private static native byte[] nativeRead(long handle, long generation, long offset, int size) throws IOException;
    private static native long nativeSize(long handle, long generation) throws IOException;
    private static native void nativeClose(long handle, long generation);
}
