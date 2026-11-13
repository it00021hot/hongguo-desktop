package com.hongguo.nsh;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import android.os.Handler;
import android.os.Looper;
import android.view.View;
import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.util.HashMap;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class FileExportComponent implements NshComponent {
    static final int REQUEST_CODE = 4101;
    private static final int COPY_BUFFER_BYTES = 64 * 1024;
    private final Activity activity;
    private final NativeEventSink context;
    private final View view;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final ExecutorService worker = Executors.newSingleThreadExecutor();
    private File source;
    private long serial;
    private volatile boolean destroyed;

    public FileExportComponent(Activity activity, NshComponentContext context) {
        this(activity, context::emit);
    }

    public FileExportComponent(Activity activity, NativeEventSink context) {
        this.activity = activity;
        this.context = context;
        this.view = new View(activity);
    }

    @Override public View view() { return view; }

    @Override public void setProp(String name, Object value) {
        if (!name.equals("request") || value == null) return;
        if (!(value instanceof Map<?, ?> request)
                || !(request.get("serial") instanceof Number version)
                || !(request.get("path") instanceof String path)) {
            android.util.Log.e("HongguoExport", "Invalid file export request");
            return;
        }
        if (source != null) {
            respond(version.longValue(), false, "A file export is already in progress");
            return;
        }
        serial = version.longValue();
        try {
            File file = new File(path).getCanonicalFile();
            String root = activity.getFilesDir().getCanonicalPath() + File.separator;
            if (!file.getPath().startsWith(root) || !file.isFile()) {
                throw new IOException("The export source is not an app file");
            }
            source = file;
            Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT)
                .addCategory(Intent.CATEGORY_OPENABLE)
                .setType("video/mp4")
                .putExtra(Intent.EXTRA_TITLE, file.getName());
            launch(intent);
        } catch (IOException | RuntimeException error) {
            source = null;
            respond(serial, false, error.toString());
        }
    }

    @SuppressWarnings("deprecation")
    private void launch(Intent intent) { activity.startActivityForResult(intent, REQUEST_CODE); }

    public void result(int resultCode, Intent data) {
        if (source == null || destroyed) return;
        long requestSerial = serial;
        if (resultCode != Activity.RESULT_OK) {
            source = null;
            respond(requestSerial, true, null);
            return;
        }
        Uri destination = data == null ? null : data.getData();
        if (destination == null) {
            source = null;
            respond(requestSerial, false, "The system did not provide an export destination");
            return;
        }
        File file = source;
        worker.execute(() -> {
            String failure = copy(file, destination);
            handler.post(() -> {
                source = null;
                if (!destroyed) respond(requestSerial, false, failure);
            });
        });
    }

    private String copy(File file, Uri destination) {
        try (FileInputStream input = new FileInputStream(file);
             OutputStream output = activity.getContentResolver().openOutputStream(destination, "w")) {
            if (output == null) throw new IOException("The export destination could not be opened");
            byte[] buffer = new byte[COPY_BUFFER_BYTES];
            int count;
            while ((count = input.read(buffer)) != -1) {
                if (destroyed || Thread.currentThread().isInterrupted()) throw new IOException("The file export was cancelled");
                output.write(buffer, 0, count);
            }
            output.flush();
            return null;
        } catch (IOException | RuntimeException error) {
            String reason = error.toString();
            try {
                activity.getContentResolver().delete(destination, null, null);
            } catch (RuntimeException cleanupError) {
                reason += "; incomplete export cleanup failed: " + cleanupError;
            }
            return reason;
        }
    }

    private void respond(long requestSerial, boolean cancelled, String error) {
        Map<String, Object> response = new HashMap<>();
        response.put("serial", requestSerial);
        response.put("isCancelled", cancelled);
        response.put("error", error);
        context.emit("completed", response);
    }

    @Override public void destroy() {
        destroyed = true;
        worker.shutdownNow();
    }
}
