/** 播放器控制栏自绘图标（B 站播放器形态）：弹幕开关 / 弹幕设置 / 网页全屏 / 全屏 / 小窗。
 *
 * 为什么不用 lucide：那套线框与 B 站播放器不是一种语言——弹幕是「弹」字、
 * 网页全屏是方框+内侧四角、小窗是画中画（Lucide 的 Frame/Settings2/MessageSquareText
 * 语义与形态都对不上，用户实测「图标太丑」）。做法与互动栏的抖音系自绘图标一致：
 * 内嵌几何、`stroke/fill` 全 `currentColor`，颜色由外层按钮的 `text-*` 接管。
 *
 * GPUI 壳的 `crates/hongguo-gpui/src/home/player_icons.rs` 是同一份几何的移植
 * （几何图标内嵌同一组 SVG，弹幕字形同尺寸同字重），改动两边一起改。
 *
 * 尺寸：控制栏按钮是 32px 方钮，图标统一 16px（与相邻 lucide 图标同格）。
 */
import { Settings } from 'lucide-react';

import { cn } from '@/lib/utils';

/** 共用描边参数：24 单位画布、1.8 描边、圆头圆角（B 站那套的粗细）。 */
const STROKE = {
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 1.8,
  strokeLinecap: 'round',
  strokeLinejoin: 'round',
} as const;

/** 图标格：16px 方框。 */
const BOX = 'size-4';

/** 弹幕开关：B 站同款「弹」字；关闭时加一道反斜杠（颜色由外层压暗）。
 *  「弹」是图标字形不是文案（B 站播放器原样），不走 i18n。
 *  无边框：与设置钮同字形，靠「有没有齿轮角标」区分，方框在 16px 下只是噪点。 */
export function DanmakuSwitchIcon({
  off = false,
  className,
}: {
  off?: boolean;
  className?: string;
}) {
  return (
    <span
      aria-hidden
      className={cn(
        BOX,
        'relative inline-grid place-items-center text-[11px] leading-none font-semibold select-none',
        className,
      )}
    >
      弹
      {off && (
        <svg viewBox="0 0 24 24" className="absolute -top-px -left-px size-[18px]" {...STROKE}>
          <path d="M3 3 21 21" />
        </svg>
      )}
    </span>
  );
}

/** 弹幕设置：无框「弹」字 + 右下角齿轮角标——与开关同字形，靠角标区分
 *  （两个相邻按钮都只写字，再加斜杠就是「关」；方框在 16px 下只是噪点）。
 *  角标垫一层深色圆底：否则会压在「弹」的右竖笔上，16px 下糊成一团。 */
export function DanmakuSettingsIcon({ className }: { className?: string }) {
  return (
    <span aria-hidden className={cn(BOX, 'relative inline-block', className)}>
      <span className="absolute inset-0 grid place-items-center text-[11px] leading-none font-semibold select-none">
        弹
      </span>
      <span className="absolute -right-[4px] -bottom-[4px] grid size-[9px] place-items-center rounded-full bg-black/70">
        <Settings className="size-[7px]" strokeWidth={2.8} />
      </span>
    </span>
  );
}

/** 网页全屏（本项目的「无边框」）：方框 + 内侧四角——B 站网页全屏同款。 */
export function WebFullscreenIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden className={cn(BOX, className)} {...STROKE}>
      <rect x="2.6" y="4.6" width="18.8" height="14.8" rx="3.4" />
      <path d="M7.5 9.7V8.4a.9.9 0 0 1 .9-.9h1.3" />
      <path d="M16.5 9.7V8.4a.9.9 0 0 0-.9-.9h-1.3" />
      <path d="M7.5 14.3v1.3a.9.9 0 0 0 .9.9h1.3" />
      <path d="M16.5 14.3v1.3a.9.9 0 0 1-.9.9h-1.3" />
    </svg>
  );
}

/** 全屏 / 退出全屏：四角折线，`exit` 时收拢（与 B 站同款的两态）。 */
export function FullscreenIcon({
  exit = false,
  className,
}: {
  exit?: boolean;
  className?: string;
}) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden className={cn(BOX, className)} {...STROKE}>
      {exit ? (
        <>
          <path d="M8.6 3.4v2.8a2.4 2.4 0 0 1-2.4 2.4H3.4" />
          <path d="M15.4 3.4v2.8a2.4 2.4 0 0 0 2.4 2.4h2.8" />
          <path d="M3.4 15.4h2.8a2.4 2.4 0 0 1 2.4 2.4v2.8" />
          <path d="M20.6 15.4h-2.8a2.4 2.4 0 0 0-2.4 2.4v2.8" />
        </>
      ) : (
        <>
          <path d="M9.4 3.4H6A2.6 2.6 0 0 0 3.4 6v3.4" />
          <path d="M14.6 3.4H18A2.6 2.6 0 0 1 20.6 6v3.4" />
          <path d="M9.4 20.6H6A2.6 2.6 0 0 1 3.4 18v-3.4" />
          <path d="M14.6 20.6H18a2.6 2.6 0 0 0 2.6-2.6v-3.4" />
        </>
      )}
    </svg>
  );
}

/** 小窗：屏幕 + 右下角小屏 + 指入箭头（B 站画中画）。 */
export function MiniWindowIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden className={cn(BOX, className)} {...STROKE}>
      <path d="M20.6 9.4V6a2.4 2.4 0 0 0-2.4-2.4H5.8A2.4 2.4 0 0 0 3.4 6v9.6a2.4 2.4 0 0 0 2.4 2.4h3.2" />
      <rect x="12.1" y="12.7" width="8.5" height="6.7" rx="2" />
      <path d="M7.3 8.3l3 3" />
      <path d="M10.3 8.7v2.6H7.7" />
    </svg>
  );
}
