/** HEVC 引导卡的会话记号（模块级）：会话语义 = 前端进程生命周期，切剧/迷你窗重挂不影响、重启重置；
 *  放独立模块是因为 react-refresh 不允许组件文件兼导出普通函数。 */

/** floating 形态「暂不安装」：切剧/迷你窗重挂不再重复打扰 */
let dismissedThisSession = false;
/** blocking 形态「转兼容格式」的会话授权：选过后连播径直走兜底，不再问 */
let compatAuthorizedThisSession = false;

export const hevcSession = {
  markDismissed(): void {
    dismissedThisSession = true;
  },
  isDismissed(): boolean {
    return dismissedThisSession;
  },
  /** 用户选了「暂不安装，转兼容格式」，或 Android 软解判假：本会话起播径直兼容、不再问 */
  authorizeCompat(): void {
    compatAuthorizedThisSession = true;
  },
  compatAuthorized(): boolean {
    return compatAuthorizedThisSession;
  },
};
