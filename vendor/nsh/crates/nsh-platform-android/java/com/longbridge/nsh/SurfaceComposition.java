package com.longbridge.nsh;

import android.view.View;
import android.view.ViewGroup;
import android.graphics.Rect;

/** Conservative admission for a native Surface below the NSH window. */
public final class SurfaceComposition {
    private SurfaceComposition() {}
    public static boolean supports(View child) {
        Rect video = new Rect(), scratch = new Rect();
        if (!child.getGlobalVisibleRect(video)) return false;
        for (View view = child; view != null; view = view.getParent() instanceof View ? (View) view.getParent() : null) {
            if (view.getAlpha() != 1f || !view.getMatrix().isIdentity()) return false;
            if (view instanceof Box && !((Box) view).supportsSeparateSurface()) return false;
            if (view instanceof NshView && ((NshView) view).screen != null) return false;
            if (view.getParent() instanceof ViewGroup) {
                ViewGroup parent = (ViewGroup) view.getParent();
                for (int index = 0; index < parent.getChildCount(); index++) {
                    View sibling = parent.getChildAt(index);
                    if (sibling != view && unsafeOverlay(sibling, video, scratch, false)) return false;
                }
            }
        }
        return child.isAttachedToWindow();
    }
    private static boolean unsafeOverlay(View view, Rect video, Rect scratch, boolean inherited) {
        if (view.getVisibility() != View.VISIBLE || view.getAlpha() == 0f) return false;
        boolean unsafe = inherited || view.getAlpha() != 1f || !view.getMatrix().isIdentity()
                || view instanceof Box && !((Box) view).supportsSeparateSurfaceOverlay();
        if (unsafe && view.getGlobalVisibleRect(scratch) && Rect.intersects(video, scratch)) return true;
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int index = 0; index < group.getChildCount(); index++)
                if (unsafeOverlay(group.getChildAt(index), video, scratch, unsafe)) return true;
        }
        return false;
    }
}
