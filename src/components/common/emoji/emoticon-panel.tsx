/**
 * 表情面板（官方同款四档：系统表情 / 第三方 GIF / 自定义表情 / 搜索）。
 *
 * 2026-10-25 官方 App 7.3.9.32 实抓对齐：
 * - **系统表情**（😀 档）：App 内置，走本地 webp + `[名字]` 文本码——发送
 *   时仍是纯文本（实测 `text:"[微笑]"`），服务端无表情字段；
 * - **第三方 GIF 档**：服务端 `search_type=2` 下发，一页 100 条，官方面板
 *   顶部标注「内容来自于第三方」；选中后作为**附图**发送
 *   （`image_data` + `emoji_scene_type=2`）；
 * - **自定义表情档**（♡ 档）：服务端 `search_type=4&keyword=profile` 下发
 *   「我的表情」（我上传的 + 我收藏的同一列表），面板内有「上传自定义
 *   表情」入口（上传 → 收藏两步）；
 * - **搜索档**：服务端 `search_type=1`，官方逐字前缀匹配。
 *
 * 长按（官方同款 500ms 阈值）：第三方/搜索结果 → 「添加到表情」（收藏
 * 进 ♡ 档）；♡ 档里的我的表情 → 「删除表情」。系统表情是本地 webp、
 * 没有服务端 id，官方不可收藏，故不挂长按。
 */
import { useState } from 'react';
import { Heart, Loader2, Search, Smile } from 'lucide-react';
import { toast } from 'sonner';

import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { t } from '@/locales';
import { cn } from '@/lib/utils';
import {
  useCollectEmoticon,
  useDeleteEmoticon,
  useEmoticonSearch,
  useOwnEmoticons,
  useThirdPartyEmoticons,
  useUploadCustomEmoticon,
} from '@/service/queries';
import type { Emoticon } from '@/service/schema';
import { localFromPicked, type LocalImageAttachment } from '@/utils/image-attachment';
import { emoticon } from '@/service/commands/emoticon';
import { DANMAKU_EMOJI_LIST } from '@/utils/danmaku-emoji';
import { EmoticonActionsMenu } from './emoticon-actions-menu';
import { EmoticonCell } from './emoticon-cell';
import type { LongPressPoint } from './use-long-press';

/** 附图场景值（官方 `business_param.emoji_scene_type`）。 */
const SCENE_THIRD_PARTY = 2;
const SCENE_OWN = 4;

type Tab = 'system' | 'thirdParty' | 'custom' | 'search';

interface Props {
  /** 系统表情（文本码）选中——插入 `[名字]`。 */
  onPickEmoji: (name: string) => void;
  /** 表情图片选中——作为附图发送（第三方 2 / 自定义 4）。 */
  onPickImage: (item: Emoticon, sceneType: number) => void;
  /** 是否支持附图（不支持时只留系统表情档——上层没接 onPickImage 就不出档位）。 */
  allowImages: boolean;
}

/** 长按弹出的操作菜单状态。 */
interface LongPressMenu {
  item: Emoticon;
  point: LongPressPoint;
  /** 该表情已在「我的表情」里 → 给「删除表情」而不是「添加到表情」。 */
  canRemove: boolean;
}

export function EmoticonPanel({ onPickEmoji, onPickImage, allowImages }: Props) {
  const [tab, setTab] = useState<Tab>('system');
  const [keyword, setKeyword] = useState('');
  const [menu, setMenu] = useState<LongPressMenu | null>(null);
  const [removeTarget, setRemoveTarget] = useState<Emoticon | null>(null);
  const thirdParty = useThirdPartyEmoticons(tab === 'thirdParty');
  const own = useOwnEmoticons(tab === 'custom');
  const search = useEmoticonSearch(keyword, tab === 'search');
  const collect = useCollectEmoticon();
  const remove = useDeleteEmoticon();
  const upload = useUploadCustomEmoticon();

  /** 上传自定义表情：选图 → 上传 → 收藏（官方两步）。选图走原生文件面板。 */
  const onUploadCustom = async (local: LocalImageAttachment) => {
    try {
      await upload.mutateAsync({
        bytes: local.bytes,
        filename: local.filename,
        width: local.width,
        height: local.height,
      });
      URL.revokeObjectURL(local.previewUrl);
      toast.success(t('player.emoticon.uploadDone'));
    } catch (e) {
      toast.error(String(e));
    }
  };

  /** 收藏一张已有表情（官方：长按 → 「添加到表情」）。 */
  const onAddToMine = async (item: Emoticon) => {
    setMenu(null);
    try {
      await collect.mutateAsync({
        imageId: item.id ?? '',
        imageUrl: item.webUri,
        width: item.width,
        height: item.height,
        imageType: item.imageType,
      });
      toast.success(t('player.emoticon.addDone'));
    } catch (e) {
      toast.error(String(e));
    }
  };

  /** 删除自己的表情（官方：长按 → 「删除表情」→ 二次确认）。 */
  const onConfirmRemove = async () => {
    const target = removeTarget;
    setRemoveTarget(null);
    if (!target) return;
    try {
      await remove.mutateAsync({
        imageId: target.id ?? '',
        imageUrl: target.webUri,
        width: target.width,
        height: target.height,
        imageType: target.imageType,
      });
      toast.success(t('player.emoticon.removeDone'));
    } catch (e) {
      toast.error(String(e));
    }
  };

  return (
    <div className="flex w-72 flex-col gap-2">
      {/* 档位切换（官方同款四档图标；不支持附图的上层只留系统表情档） */}
      <div className="flex items-center gap-1 border-b border-white/10 pb-1.5">
        {allowImages && (
          <TabButton
            active={tab === 'search'}
            label={t('player.emoticon.search')}
            onClick={() => setTab('search')}
          >
            <Search className="size-4" aria-hidden />
          </TabButton>
        )}
        <TabButton
          active={tab === 'system'}
          label={t('player.emoticon.system')}
          onClick={() => setTab('system')}
        >
          <Smile className="size-4" aria-hidden />
        </TabButton>
        {allowImages && (
          <>
            <TabButton
              active={tab === 'custom'}
              label={t('player.emoticon.custom')}
              onClick={() => setTab('custom')}
            >
              <Heart className="size-4" aria-hidden />
            </TabButton>
            <TabButton
              active={tab === 'thirdParty'}
              label={t('player.emoticon.thirdParty')}
              onClick={() => setTab('thirdParty')}
            >
              <span className="text-[10px] font-semibold tracking-tight">GIF</span>
            </TabButton>
          </>
        )}
      </div>

      {tab === 'search' && (
        <input
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
          placeholder={t('player.emoticon.searchPlaceholder')}
          className="h-8 rounded-md bg-neutral-800/80 px-2.5 text-xs text-white outline-none placeholder:text-neutral-500 focus:ring-1 focus:ring-white/20"
        />
      )}
      {tab === 'system' && (
        <div className="grid max-h-52 scrollbar-thin grid-cols-8 gap-0.5 overflow-y-auto">
          {DANMAKU_EMOJI_LIST.map((e) => (
            <button
              key={e.name}
              type="button"
              title={e.name}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => onPickEmoji(e.name.slice(1, -1))}
              className="grid cursor-pointer place-items-center rounded-md p-1 transition-colors hover:bg-white/15"
            >
              <img src={e.url} alt={e.name} draggable={false} className="size-6 object-contain" />
            </button>
          ))}
        </div>
      )}

      {tab === 'thirdParty' && (
        <>
          <p className="text-[11px] text-neutral-400">{t('player.emoticon.thirdPartyNote')}</p>
          <EmoticonGrid
            items={thirdParty.data?.pages.flatMap((p) => p.items) ?? []}
            loading={thirdParty.isPending}
            onPick={(item) => onPickImage(item, SCENE_THIRD_PARTY)}
            onMenu={(item, point) => setMenu({ item, point, canRemove: false })}
          />
          {thirdParty.hasNextPage && (
            <button
              type="button"
              onClick={() => void thirdParty.fetchNextPage()}
              disabled={thirdParty.isFetchingNextPage}
              className="cursor-pointer rounded-md py-1 text-[11px] text-neutral-400 hover:bg-white/10"
            >
              {t('player.emoticon.loadMore')}
            </button>
          )}
        </>
      )}

      {tab === 'custom' && (
        <>
          <div className="grid max-h-52 scrollbar-thin grid-cols-8 gap-0.5 overflow-y-auto">
            <button
              type="button"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                void emoticon.pickImage().then((picked) => {
                  const local = localFromPicked(picked);
                  if (local) void onUploadCustom(local);
                });
              }}
              disabled={upload.isPending}
              title={t('player.emoticon.uploadCustom')}
              className="grid aspect-square cursor-pointer place-items-center rounded-md border border-dashed border-white/25 text-neutral-300 transition-colors hover:bg-white/10"
            >
              {upload.isPending ? (
                <Loader2 className="size-4 animate-spin" aria-hidden />
              ) : (
                <span className="text-lg leading-none">+</span>
              )}
            </button>
            {(own.data?.items ?? []).map((item) => (
              <EmoticonCell
                key={item.id ?? item.webUri}
                item={item}
                onPick={() => onPickImage(item, SCENE_OWN)}
                onMenu={(target, point) => setMenu({ item: target, point, canRemove: true })}
              />
            ))}
          </div>
          {(own.data?.items.length ?? 0) === 0 && !own.isPending && (
            <p className="text-[11px] text-neutral-500">{t('player.emoticon.customEmpty')}</p>
          )}
        </>
      )}

      {tab === 'search' && (
        // 没输入就不出网格——「没有找到表情」只属于真实搜索后无结果，
        // 空关键词显示它是误导（GPUI 壳同口径）。
        keyword.trim() !== '' && (
          <EmoticonGrid
            items={search.data?.items ?? []}
            // enabled=false（无关键词）时 isPending 恒 true，不能当 loading
            // 画无限转圈——没输入就保持空态，输入后只认真实在飞的请求。
            loading={search.isFetching}
            onPick={(item) => onPickImage(item, SCENE_THIRD_PARTY)}
            onMenu={(item, point) => setMenu({ item, point, canRemove: false })}
          />
        )
      )}

      {/* 长按操作菜单（官方同款：添加到表情 / 删除表情） */}
      {menu && (
        <EmoticonActionsMenu
          x={menu.point.x}
          y={menu.point.y}
          canRemove={menu.canRemove}
          onAdd={() => void onAddToMine(menu.item)}
          onRemove={() => {
            setMenu(null);
            setRemoveTarget(menu.item);
          }}
          onClose={() => setMenu(null)}
        />
      )}
      <ConfirmDialog
        open={!!removeTarget}
        onOpenChange={(open) => !open && setRemoveTarget(null)}
        title={t('player.emoticon.removeConfirmTitle')}
        description={t('player.emoticon.removeConfirmDesc')}
        confirmLabel={t('common.delete')}
        onConfirm={() => void onConfirmRemove()}
      />
    </div>
  );
}

function TabButton({
  active,
  label,
  onClick,
  children,
}: {
  active: boolean;
  label: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      className={cn(
        'grid size-7 place-items-center rounded-full text-neutral-300 transition-colors hover:bg-white/10',
        active && 'bg-white/15 text-white',
      )}
    >
      {children}
    </button>
  );
}

function EmoticonGrid({
  items,
  loading,
  onPick,
  onMenu,
}: {
  items: Emoticon[];
  loading: boolean;
  onPick: (item: Emoticon) => void;
  onMenu?: (item: Emoticon, point: LongPressPoint) => void;
}) {
  if (loading) {
    return (
      <div className="grid place-items-center py-6 text-neutral-400">
        <Loader2 className="size-4 animate-spin" aria-hidden />
      </div>
    );
  }
  if (items.length === 0) {
    return (
      <p className="py-6 text-center text-[11px] text-neutral-500">{t('player.emoticon.empty')}</p>
    );
  }
  return (
    <div className="grid max-h-52 scrollbar-thin grid-cols-6 gap-1 overflow-y-auto">
      {items.map((item, index) => (
        <EmoticonCell
          key={item.id ?? `${item.webUri}-${index}`}
          item={item}
          onPick={onPick}
          onMenu={onMenu}
        />
      ))}
    </div>
  );
}
