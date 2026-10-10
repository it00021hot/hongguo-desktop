/** 沉浸流信息叠加层：@剧名/热度/集数/简介压在画面左下（纯 props→JSX）。 */
import type { Dispatch, SetStateAction } from 'react';
import type { UseNavigateResult } from '@tanstack/react-router';
import { cn } from '@/lib/utils';
import { t, tf } from '@/locales';

/**
 * 沉浸流信息叠加（hgplayer 同款）：@剧名/集数/简介压在画面左下。
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
}) {
  return (
    <div
      className={cn(
        'absolute left-3 z-10 max-w-[62%] transition-all duration-300',
        chromeShown && !miniScreen
          ? 'bottom-28 opacity-100'
          : 'pointer-events-none bottom-14 opacity-0',
      )}
    >
      {/* 热度行（hgplayer 1.1.6 同款：剧名上方） */}
      {overlayMeta?.heatText && (
        <p className="text-xs font-medium text-amber-300/90 drop-shadow-md">
          {overlayMeta.heatText}
        </p>
      )}
      {/* 剧名 → 详情页。第三方同款交互：点标题离开播放器看档案/选集。
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
        // 简介块只占舞台约三分之一（hgplayer 同款量级，大屏实测
        // ~400px）：之前跟着容器吃到 62%，两行密文糊满左下角。
        // 容器吃掉点击（防触发舞台暂停/继续），展开交互只在按钮上
        <div className="mt-1 flex max-w-[36%] items-end gap-2" onClick={(e) => e.stopPropagation()}>
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
    </div>
  );
}
