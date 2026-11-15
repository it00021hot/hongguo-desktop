package com.hongguo.nsh;

import android.app.Activity;
import android.content.Intent;
import android.content.pm.ActivityInfo;
import android.os.Bundle;
import com.longbridge.nsh.NshView;

public final class MainActivity extends Activity {
    static {
        System.loadLibrary("nsh");
    }

    private NshView view;
    private FileExportComponent fileExport;
    private static native void initialize(String dataRoot);

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        setRequestedOrientation(ActivityInfo.SCREEN_ORIENTATION_SENSOR_PORTRAIT);
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
        if (view != null) view.destroy();
        super.onDestroy();
    }
}
