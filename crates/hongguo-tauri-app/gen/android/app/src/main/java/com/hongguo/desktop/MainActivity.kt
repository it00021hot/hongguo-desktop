package com.hongguo.desktop

import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.util.TypedValue
import android.view.WindowInsets
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  /** onWebViewCreate 存下的 WebView 引用：返回键桥与 insets 都挂在它上面。 */
  private var webViewRef: WebView? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
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
  // 盖住点不了，底部导航条被手势条压着。把 systemBars insets 折成 WebView
  // 的布局 margin——**不能用 padding**：WebView（Chromium）不支持 padding，
  // 网页内容照旧铺满 view，状态栏照样叠在顶栏上（真机实测）。margin 收缩
  // view 本体，内容区避开系统栏，系统栏区域露出主题窗口底色。返回 insets
  // 不消费，Chromium 子树的 insets 分发照常。
  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    webViewRef = webView
    webView.setBackgroundColor(surfaceBackgroundColor())
    webView.setOnApplyWindowInsetsListener { v, insets ->
      val (left, top, right, bottom) =
          if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val i = insets.getInsets(WindowInsets.Type.systemBars())
            listOf(i.left, i.top, i.right, i.bottom)
          } else {
            @Suppress("DEPRECATION")
            listOf(
                insets.systemWindowInsetLeft,
                insets.systemWindowInsetTop,
                insets.systemWindowInsetRight,
                insets.systemWindowInsetBottom,
            )
          }
      val lp = v.layoutParams as? android.widget.FrameLayout.LayoutParams
      if (lp != null && (lp.topMargin != top || lp.bottomMargin != bottom || lp.leftMargin != left || lp.rightMargin != right)) {
        lp.setMargins(left, top, right, bottom)
        v.layoutParams = lp
      }
      insets
    }
  }

  /** WebView 背景对齐主题昼夜底色：insets padding 区露出的就是它。 */
  private fun surfaceBackgroundColor(): Int {
    val tv = TypedValue()
    return if (theme.resolveAttribute(android.R.attr.colorBackground, tv, true)) tv.data
    else Color.WHITE
  }
}
