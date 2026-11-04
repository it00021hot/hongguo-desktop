package com.hongguo.desktop

import android.content.pm.ActivityInfo
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.util.TypedValue
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import kotlin.math.max

class MainActivity : TauriActivity() {
  /** onWebViewCreate 存下的 WebView 引用：返回键桥与 insets 都挂在它上面。 */
  private var webViewRef: WebView? = null
  /** 上次推给前端的 IME 抬升（CSS px），去重避免每帧 evaluateJavascript。 */
  private var lastImeCssPx = -1

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    // 信息流是竖滑形态：默认锁竖屏；「全屏观看」再临时切横屏
    requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_PORTRAIT
    // edge-to-edge 下 adjustResize 本身不缩内容，但仍声明：旧机 + 配合 IME insets 分发
    window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)
    super.onCreate(savedInstanceState)
  }

  // 系统返回（手势侧滑/返回键）：优先交给前端 web 路由回退。前端桥返回
  // true = 本次返回已被消费（回退了，或在栈底转「退出确认」框自己接管）；
  // 桥不存在（前端还没起来/异常）才走默认退出，保证永远有出路。
  override fun onBackPressed() {
    val wv = webViewRef
    if (wv == null) {
      super.onBackPressed()
      return
    }
    wv.evaluateJavascript(
        "(window.__hgAndroidBack && window.__hgAndroidBack()) ? 'consumed' : 'exit'"
    ) { result ->
      if (result == "\"exit\"") {
        finishAffinity()
      }
    }
  }

  // enableEdgeToEdge 让 WebView 铺到状态栏/导航条底下：顶栏 tab 被状态栏
  // 盖住点不了，底部导航条被手势条压着。systemBars 折成 WebView margin
  // （**不能用 padding**：WebView/Chromium 不支持）。
  // IME 不进 margin——edge-to-edge 下软键盘常盖住而不缩 WebView；改由
  // `__hgSetImeBottom(cssPx)` 推给前端，发弹幕输入框 `position:fixed; bottom` 顶起。
  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    webViewRef = webView
    webView.setBackgroundColor(surfaceBackgroundColor())
    webView.addJavascriptInterface(AndroidBridge(this), "HongguoAndroid")
    ViewCompat.setOnApplyWindowInsetsListener(webView) { v, windowInsets ->
      val bars = windowInsets.getInsets(WindowInsetsCompat.Type.systemBars())
      val ime = windowInsets.getInsets(WindowInsetsCompat.Type.ime())
      val lp = v.layoutParams as? android.widget.FrameLayout.LayoutParams
      if (lp != null &&
          (lp.topMargin != bars.top ||
              lp.bottomMargin != bars.bottom ||
              lp.leftMargin != bars.left ||
              lp.rightMargin != bars.right)) {
        lp.setMargins(bars.left, bars.top, bars.right, bars.bottom)
        v.layoutParams = lp
      }
      // 键盘顶相对 WebView 底边的抬升：IME 已含导航区，要扣掉已进 margin 的 bars.bottom
      val liftPx = max(0, ime.bottom - bars.bottom)
      pushImeBottomCss(webView, liftPx)
      windowInsets
    }
    ViewCompat.requestApplyInsets(webView)
  }

  /** 物理像素 → CSS px，推给前端 `__hgSetImeBottom`。 */
  private fun pushImeBottomCss(webView: WebView, liftPx: Int) {
    val density = resources.displayMetrics.density
    if (density <= 0f) return
    val cssPx = Math.round(liftPx / density)
    if (cssPx == lastImeCssPx) return
    lastImeCssPx = cssPx
    webView.post {
      webView.evaluateJavascript(
          "window.__hgSetImeBottom&&window.__hgSetImeBottom($cssPx)",
          null,
      )
    }
  }

  /** WebView 背景对齐主题昼夜底色：insets padding 区露出的就是它。 */
  private fun surfaceBackgroundColor(): Int {
    val tv = TypedValue()
    return if (theme.resolveAttribute(android.R.attr.colorBackground, tv, true)) tv.data
    else Color.WHITE
  }

  /**
   * 给前端的横屏锁：全屏观看横片时锁 SENSOR_LANDSCAPE + 藏导航条，退出回
   * SENSOR_PORTRAIT 并恢复导航条（信息流竖滑，不能停在横屏）。
   */
  fun setLandscapeLocked(on: Boolean) {
    requestedOrientation =
        if (on) ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        else ActivityInfo.SCREEN_ORIENTATION_SENSOR_PORTRAIT
    setNavigationBarHidden(on)
  }

  /** 横屏全屏藏导航条（状态栏保留，对齐官方控件顶栏）；竖屏信息流恢复。 */
  private fun setNavigationBarHidden(hidden: Boolean) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      val controller = window.insetsController ?: return
      if (hidden) {
        controller.hide(WindowInsets.Type.navigationBars())
        controller.systemBarsBehavior =
            WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
      } else {
        controller.show(WindowInsets.Type.navigationBars())
      }
      return
    }
    @Suppress("DEPRECATION")
    window.decorView.systemUiVisibility =
        if (hidden) {
          (View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
              or View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
              or View.SYSTEM_UI_FLAG_LAYOUT_STABLE
              or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION)
        } else {
          View.SYSTEM_UI_FLAG_VISIBLE
        }
  }
}

/** JS ↔ Kotlin 桥：前端 `window.HongguoAndroid.setLandscape(true/false)`。 */
class AndroidBridge(private val activity: MainActivity) {
  @JavascriptInterface
  fun setLandscape(on: Boolean) {
    activity.runOnUiThread { activity.setLandscapeLocked(on) }
  }
}
