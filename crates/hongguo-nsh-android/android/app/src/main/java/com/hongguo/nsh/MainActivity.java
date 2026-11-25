package com.hongguo.nsh;

import android.app.Activity;
import android.content.Intent;
import android.content.pm.ActivityInfo;
import android.os.Bundle;
import android.system.ErrnoException;
import android.system.Os;
import android.view.Window;
import android.view.WindowManager;
import com.longbridge.nsh.NshView;
import java.lang.ref.WeakReference;

public final class MainActivity extends Activity {
    static {
        System.loadLibrary("nsh");
    }

    private static WeakReference<MainActivity> current;
    private NshView view;
    private FileExportComponent fileExport;
    private static native void initialize(String dataRoot);

    public static void setKeepScreenOn(boolean on) {
        MainActivity activity = current == null ? null : current.get();
        if (activity == null) {
            return;
        }
        activity.runOnUiThread(() -> applyKeepScreenOn(activity, on));
    }

    private static void applyKeepScreenOn(MainActivity activity, boolean on) {
        if (activity.isDestroyed()) {
            return;
        }
        Window window = activity.getWindow();
        if (window == null) {
            return;
        }
        if (on) {
            window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        } else {
            window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        }
    }

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        current = new WeakReference<>(this);
        setRequestedOrientation(ActivityInfo.SCREEN_ORIENTATION_SENSOR_PORTRAIT);
        try {
            Os.setenv("TMPDIR", getCacheDir().getAbsolutePath(), true);
        } catch (ErrnoException error) {
            throw new IllegalStateException("Cannot configure application temporary directory", error);
        }
        initialize(getFilesDir().getAbsolutePath());
        view = new NshView(this);
        view.registerComponent("hongguo.window-modes", context -> new WindowModeComponent(this, context));
        view.registerComponent("hongguo.file-export", context -> {
            fileExport = new FileExportComponent(this, context);
            return fileExport;
        });
        view.registerComponent("hongguo.pixel-video", context -> new PixelVideoComponent(this, context));
        setContentView(view);
        view.runRustApp("nsh");
        view.handleNotificationIntent(getIntent());
    }

    @Override
    @SuppressWarnings("deprecation")
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == FileExportComponent.REQUEST_CODE && fileExport != null) {
            fileExport.result(resultCode, data);
        }
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        if (view != null) view.handleNotificationIntent(intent);
    }

    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        if (view == null || !view.handleBack()) super.onBackPressed();
    }

    @Override
    protected void onDestroy() {
        applyKeepScreenOn(this, false);
        if (view != null) view.destroy();
        MainActivity attached = current == null ? null : current.get();
        if (attached == this) {
            current.clear();
        }
        super.onDestroy();
    }
}
