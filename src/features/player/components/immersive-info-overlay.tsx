/** 沉浸流信息叠加层：@剧名/热度/集数/简介压在画面左下（纯 props→JSX）；
 *  左上角两枚 pill：选中剧连播时亮「< 第N集」，点返回解除连播回到换剧
 *  （手机端同款）；有外部来源页（returnTo）时亮「< 返回」，点它回来源页
 *  ——从历史/搜索进来就回历史/搜索，而不是停在推荐流。 */
import type { Dispatch, SetStateAction } from 'react';
import type { UseNavigateResult } from '@tanstack/react-router';
import { ChevronLeft } from 'lucide-react';
import { cn } from '@/lib/utils';
import { t, tf } from '@/locales';
import { ChromeSurface } from './chrome-surface';

/** 左上角 pill 的共用皮肤（播放器 chrome 黑玻璃，与控制栏一族）。 */
const PILL_CLASS =
  'flex h-6 cursor-pointer items-center gap-0.5 rounded-full bg-black/30 pr-2 pl-1 text-xs font-medium text-white drop-shadow-md backdrop-blur-sm transition-colors hover:bg-black/50';

/**
 * 左上角 pill 行：连播指示 + 返回来源页。
 *
 * 与信息叠加分开渲染：pill 是**导航出口**，取流失败/加载中的错误态也
 * 必须在——用户从历史/搜索进来撞上持续取流失败时，恰恰最需要「退回去」
 * （那时信息叠加层整个不在，Esc 也要 `<video>` 在场才接键）。
 * `always` 为真时常显（错误态），否则对齐 chromeShown 随控制栏淡入淡出。
 */
export function PlayerChromePills({
  chromeShown,
  miniScreen,
  always = false,
  inBinge,
  onExitBinge,
  vidIndex,
  returnTo,
  onReturnToSource,
}: {
  chromeShown: boolean;
  miniScreen: boolean;
  /** 常显（错误/加载态）：导航出口不随控制栏淡出 */
  always?: boolean;
  /** 选中剧连播中：亮「< 第N集」，点返回解除连播回到换剧（手机端同款） */
  inBinge?: boolean;
  onExitBinge?: () => void;
  vidIndex: number;
  /** 外部来源路由（含 search 的 href，usePlaySeries 记）：非空亮「< 返回」 */
  returnTo?: string | null;
  /** 点返回：回来源页（宿主负责导航并清掉 returnTo） */
  onReturnToSource?: () => void;
}) {
  if (!(inBinge || returnTo)) return null;
  return (
    // 显隐对齐播放控件（错误态常显）；stopPropagation：点 pill 别触发
    // 舞台的播放/暂停。
    <ChromeSurface
      shown={(always || chromeShown) && !miniScreen}
      onClick={(e) => e.stopPropagation()}
      className="absolute top-3 left-3 z-10 flex w-fit items-center gap-1.5"
    >
      {inBinge && (
        <button
          type="button"
          onClick={onExitBinge}
          title={t('player.exitBinge')}
          className={PILL_CLASS}
        >
          <ChevronLeft className="size-3.5" />
          {tf('player.episodeNo', { index: vidIndex })}
        </button>
      )}
      {returnTo && (
        <button
          type="button"
          onClick={onReturnToSource}
          title={t('player.backToSourceHint')}
          className={PILL_CLASS}
        >
          <ChevronLeft className="size-3.5" />
          {t('player.backToSource')}
        </button>
      )}
    </ChromeSurface>
  );
}

/**
 * 沉浸流信息叠加：@剧名/集数/简介压在画面左下。
 * 控制栏弹出时整体抬到控制栏上沿之上（bottom-28），隐藏时落回
 * bottom-14——两者transition 联动，不再互相遮挡。
 * 小屏模式不用这坨：紧凑控件条自带收敛的剧名行。
 *
 * 无自身状态：简介展开态（introExpanded/setIntroExpanded）与跳详情的
 * navigate 都由宿主持有、经 props 接线，本组件只做展示。
 */
export function ImmersiveInfoOverlay({
  chromeShown,
  miniScreen,
  overlayMeta,
  title,
  episodeCount,
  seriesId,
  vidIndex,
  intro,
  introExpanded,
  setIntroExpanded,
  navigate,
  inBinge,
  onExitBinge,
  returnTo,
  onReturnToSource,
}: {
  chromeShown: boolean;
  miniScreen: boolean;
  /** 信息流条目自带的展示标记（热度文本/季角标/运营角标） */
  overlayMeta?: { heatText?: string; seasonTag?: string; badge?: string };
  /** 剧名（按钮 title 提示与 @标题正文共用；档案未到时为 undefined） */
  title?: string;
  episodeCount?: number;
  seriesId: string;
  vidIndex: number;
  /** 剧集简介（video_detail 接口，宿主自取） */
  intro?: string;
  introExpanded: boolean;
  setIntroExpanded: Dispatch<SetStateAction<boolean>>;
  navigate: UseNavigateResult<string>;
  /** 选中剧连播中：左上角亮「< 第N集」 */
  inBinge?: boolean;
  /** 点返回：解除连播锁定，滚轮/↑↓ 恢复换剧 */
  onExitBinge?: () => void;
  /** 外部来源路由（含 search 的 href，usePlaySeries 记）：非空亮「< 返回」 */
  returnTo?: string | null;
  /** 点返回：回来源页（宿主负责导航并清掉 returnTo） */
  onReturnToSource?: () => void;
}) {
  return (
    <>
      <PlayerChromePills
        chromeShown={chromeShown}
        miniScreen={miniScreen}
        inBinge={inBinge}
        onExitBinge={onExitBinge}
        vidIndex={vidIndex}
        returnTo={returnTo}
        onReturnToSource={onReturnToSource}
      />
      <ChromeSurface
        shown={chromeShown && !miniScreen}
        className="absolute bottom-28 left-3 z-10 max-w-[62%] transition-all duration-300 data-[shown=false]:bottom-14"
      >
        {/* 热度行（剧名上方） */}
        {overlayMeta?.heatText && (
          <p className="text-xs font-medium text-amber-300/90 drop-shadow-md">
            {overlayMeta.heatText}
          </p>
        )}
        {/* 剧名 → 详情页。官方客户端同款交互：点标题离开播放器看档案/选集。
        stopPropagation：点标题不能同时触发「点画面暂停」。 */}
        <div className="flex items-center gap-1.5">
          {overlayMeta?.badge && (
            <span className="rounded-sm bg-red-500/90 px-1 py-px text-[10px] font-semibold text-white">
              {overlayMeta.badge}
            </span>
          )}
          {overlayMeta?.seasonTag && (
            <span className="rounded-sm bg-white/20 px-1 py-px text-[10px] font-semibold text-white">
              {overlayMeta.seasonTag}
            </span>
          )}
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              void navigate({ to: '/detail', search: { seriesId } });
            }}
            className="cursor-pointer text-sm font-semibold text-white drop-shadow-md hover:underline"
            title={title}
          >
            @{title ?? ''}
          </button>
        </div>
        <p className="mt-0.5 text-xs text-white/85 drop-shadow-md">
          {tf('player.epShort', { index: vidIndex })}
          {episodeCount != null && episodeCount > 0 && (
            <span className="text-white/70">
              {' · '}
              {tf('player.totalEpisodes', { count: episodeCount })}
            </span>
          )}
        </p>
        {intro && (
          // 简介块只占舞台约三分之一（大屏实测
          // ~400px）：之前跟着容器吃到 62%，两行密文糊满左下角。
          // 容器吃掉点击（防触发舞台暂停/继续），展开交互只在按钮上
          <div
            className="mt-1 flex max-w-[36%] items-end gap-2"
            onClick={(e) => e.stopPropagation()}
          >
            <p
              className={cn(
                'text-xs leading-relaxed text-white/70 drop-shadow-md',
                !introExpanded && 'line-clamp-2',
              )}
            >
              {intro}
            </p>
            {intro.length > 40 && (
              <button
                type="button"
                onClick={() => setIntroExpanded((v) => !v)}
                className="shrink-0 cursor-pointer pb-0.5 text-xs text-white/60 drop-shadow-md hover:text-white"
              >
                {introExpanded ? t('player.introCollapse') : t('player.introExpand')}
              </button>
            )}
          </div>
        )}
      </ChromeSurface>
    </>
  );
}
