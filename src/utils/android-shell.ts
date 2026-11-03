/**
 * Android WebView 壳桥（MainActivity JavascriptInterface `HongguoAndroid`）。
 * 桌面/iOS 无此对象，调用为空操作。
 */

declare global {
  interface Window {
    HongguoAndroid?: {
      setLandscape: (on: boolean) => void;
    };
  }
}

/** 锁/解锁 Activity 横屏（全屏观看横片用）。 */
export function setAndroidLandscape(on: boolean): void {
  window.HongguoAndroid?.setLandscape(on);
}
