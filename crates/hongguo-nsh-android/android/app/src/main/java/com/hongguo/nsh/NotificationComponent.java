package com.hongguo.nsh;

import android.Manifest;
import android.app.Activity;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.os.Build;
import android.view.View;
import com.longbridge.nsh.NshComponent;
import com.longbridge.nsh.NshComponentContext;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

public final class NotificationComponent implements NshComponent {
    private static final String CHANNEL_ID = "follow-updates";
    private static final int PERMISSION_REQUEST = 4849;
    private final Activity activity;
    private final NativeEventSink context;
    private final View view;
    private long serial;
    private Long permissionSerial;

    public NotificationComponent(Activity activity, NshComponentContext context) {
        this(activity, context::emit);
    }

    public NotificationComponent(Activity activity, NativeEventSink context) {
        this.activity = activity;
        this.context = context;
        this.view = new View(activity);
    }

    @Override public View view() { return view; }

    @Override public void setProp(String name, Object value) {
        if (!name.equals("requests") || value == null) return;
        if (!(value instanceof List<?> requests)) {
            context.emit("failure", "Invalid notification requests");
            return;
        }
        for (Object entry : requests) {
            if (!(entry instanceof Map<?, ?> request)
                    || !(request.get("serial") instanceof Number version)
                    || !(request.get("action") instanceof Map<?, ?> action)) {
                context.emit("failure", "Invalid notification request");
                continue;
            }
            long next = version.longValue();
            if (next <= serial) continue;
            serial = next;
            try {
                dispatch(next, action);
            } catch (RuntimeException error) {
                complete(next, false, error.toString());
            }
        }
    }

    private void dispatch(long version, Map<?, ?> action) {
        String kind = requireString(action, "kind");
        if (kind.equals("requestPermission")) {
            requestPermission(version);
            return;
        }
        if (!kind.equals("send")) throw new IllegalArgumentException("Unknown notification action");
        if (!allowed()) {
            complete(version, false, null);
            return;
        }
        String id = requireString(action, "id");
        String title = requireString(action, "title");
        String body = requireString(action, "body");
        NotificationManager manager = manager();
        NotificationChannel channel = new NotificationChannel(CHANNEL_ID,
                activity.getString(R.string.app_name), NotificationManager.IMPORTANCE_DEFAULT);
        channel.setSound(null, null);
        channel.enableVibration(false);
        manager.createNotificationChannel(channel);
        Intent open = new Intent(activity, MainActivity.class)
                .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
        PendingIntent content = PendingIntent.getActivity(activity, 0, open,
                PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
        Notification notification = new Notification.Builder(activity, CHANNEL_ID)
                .setSmallIcon(R.drawable.hg_notification)
                .setContentTitle(title)
                .setContentText(body)
                .setStyle(new Notification.BigTextStyle().bigText(body))
                .setContentIntent(content)
                .setAutoCancel(true)
                .setOnlyAlertOnce(true)
                .build();
        manager.notify(id, 0, notification);
        complete(version, true, null);
    }

    private void requestPermission(long version) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU
                || activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                        == PackageManager.PERMISSION_GRANTED) {
            complete(version, allowed(), null);
            return;
        }
        if (permissionSerial != null) {
            complete(version, false, "A notification permission request is already pending");
            return;
        }
        permissionSerial = version;
        activity.requestPermissions(new String[] {Manifest.permission.POST_NOTIFICATIONS}, PERMISSION_REQUEST);
    }

    public void onRequestPermissionsResult(int requestCode, int[] grantResults) {
        if (requestCode != PERMISSION_REQUEST || permissionSerial == null) return;
        long version = permissionSerial;
        permissionSerial = null;
        complete(version, grantResults.length > 0
                && grantResults[0] == PackageManager.PERMISSION_GRANTED && allowed(), null);
    }

    private NotificationManager manager() {
        return activity.getSystemService(NotificationManager.class);
    }

    private boolean allowed() {
        if (!manager().areNotificationsEnabled()) return false;
        NotificationChannel channel = manager().getNotificationChannel(CHANNEL_ID);
        return channel == null || channel.getImportance() != NotificationManager.IMPORTANCE_NONE;
    }

    private void complete(long version, boolean granted, String error) {
        Map<String, Object> response = new HashMap<>();
        response.put("serial", version);
        response.put("granted", granted);
        response.put("error", error);
        context.emit("result", response);
    }

    private static String requireString(Map<?, ?> value, String key) {
        if (value.get(key) instanceof String text) return text;
        throw new IllegalArgumentException("Missing notification field: " + key);
    }
}
