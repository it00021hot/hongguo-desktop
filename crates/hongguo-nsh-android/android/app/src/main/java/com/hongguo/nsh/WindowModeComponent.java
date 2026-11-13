package com.hongguo.nsh;

import android.app.Activity;
import android.content.pm.ActivityInfo;
import android.content.res.Configuration;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.view.View;
import android.view.ViewTreeObserver;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import java.util.HashMap;
import java.util.Map;

public final class WindowModeComponent implements NshComponent {
    private static final long CONFIRMATION_TIMEOUT_MS = 3000;
    private final Activity activity;
    private final NativeEventSink context;
    private final View view;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final ViewTreeObserver.OnGlobalLayoutListener layout = this::confirm;
    private final Runnable deadline = this::timeout;
    private long serial;
    private boolean pending;
    private boolean active;
    private boolean owned;
    private boolean targetLandscape;
    private int previousOrientation;
    private int previousConfiguration;
    private int previousVisibility;
    private boolean previousNavigationVisible;
    private int previousBarsBehavior;

    public WindowModeComponent(Activity activity, NshComponentContext context) {
        this(activity, context::emit);
    }
    public WindowModeComponent(Activity activity, NativeEventSink context) {
        this.activity = activity;
        this.context = context;
        this.view = new View(activity);
        activity.getWindow().getDecorView().getViewTreeObserver().addOnGlobalLayoutListener(layout);
    }

    @Override public View view() { return view; }

    @Override public void setProp(String name, Object value) {
        if (!name.equals("request") || value == null) return;
        if (!(value instanceof Map<?, ?> request)
                || !(request.get("serial") instanceof Number version)
                || !(request.get("landscape") instanceof Boolean landscape)) {
            context.emit("failure", "Invalid mobile window request");
            return;
        }
        serial = version.longValue();
        handler.removeCallbacks(deadline);
        pending = false;
        try {
            if (!landscape && !owned) {
                complete(null);
                return;
            }
            if (!owned) {
                previousOrientation = activity.getRequestedOrientation();
                previousConfiguration = activity.getResources().getConfiguration().orientation;
                previousVisibility = legacyVisibility();
                captureBars();
                owned = true;
            }
            targetLandscape = landscape;
            pending = true;
            activity.setRequestedOrientation(landscape
                ? ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE : previousOrientation);
            systemBars(!landscape);
            handler.postDelayed(deadline, CONFIRMATION_TIMEOUT_MS);
            confirm();
        } catch (RuntimeException error) {
            pending = false;
            complete(failedRestore(error.toString()));
        }
    }

    private void confirm() {
        int actual = activity.getResources().getConfiguration().orientation;
        if (!pending) {
            if (active && actual != Configuration.ORIENTATION_LANDSCAPE) {
                complete(failedRestore("The system changed the requested screen orientation"));
            }
            return;
        }
        int expected = targetLandscape ? Configuration.ORIENTATION_LANDSCAPE : previousConfiguration;
        if (actual != expected) return;
        active = targetLandscape;
        if (!active) owned = false;
        pending = false;
        handler.removeCallbacks(deadline);
        complete(null);
    }

    private void timeout() {
        if (!pending) return;
        pending = false;
        complete(failedRestore("The system did not allow the requested screen orientation"));
    }

    private void complete(String error) {
        Map<String, Object> response = new HashMap<>();
        response.put("serial", serial);
        response.put("landscape", active);
        response.put("error", error);
        context.emit("orientation", response);
    }

    private String failedRestore(String reason) {
        try {
            if (owned) {
                activity.setRequestedOrientation(previousOrientation);
                systemBars(true);
            }
        } catch (RuntimeException error) {
            reason += "; restore failed: " + error;
        } finally {
            owned = false;
            active = false;
        }
        return reason;
    }

    private void captureBars() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return;
        View decor = activity.getWindow().getDecorView();
        WindowInsets insets = decor.getRootWindowInsets();
        previousNavigationVisible = insets == null || insets.isVisible(WindowInsets.Type.navigationBars());
        WindowInsetsController controller = activity.getWindow().getInsetsController();
        previousBarsBehavior = controller == null ? 0 : controller.getSystemBarsBehavior();
    }

    @SuppressWarnings("deprecation")
    private int legacyVisibility() { return activity.getWindow().getDecorView().getSystemUiVisibility(); }

    @SuppressWarnings("deprecation")
    private void systemBars(boolean visible) {
        View decor = activity.getWindow().getDecorView();
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            WindowInsetsController controller = activity.getWindow().getInsetsController();
            if (controller != null) {
                if (visible) {
                    controller.setSystemBarsBehavior(previousBarsBehavior);
                    if (previousNavigationVisible) controller.show(WindowInsets.Type.navigationBars());
                    else controller.hide(WindowInsets.Type.navigationBars());
                } else {
                    controller.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
                    controller.hide(WindowInsets.Type.navigationBars());
                }
            }
        } else {
            decor.setSystemUiVisibility(visible ? previousVisibility :
                View.SYSTEM_UI_FLAG_HIDE_NAVIGATION | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY |
                View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION | View.SYSTEM_UI_FLAG_LAYOUT_STABLE);
        }
    }

    @Override public void destroy() {
        handler.removeCallbacks(deadline);
        pending = false;
        ViewTreeObserver observer = activity.getWindow().getDecorView().getViewTreeObserver();
        if (observer.isAlive()) observer.removeOnGlobalLayoutListener(layout);
        String error = failedRestore("");
        if (!error.isEmpty()) android.util.Log.w("HongguoWindowMode", error);
    }
}
