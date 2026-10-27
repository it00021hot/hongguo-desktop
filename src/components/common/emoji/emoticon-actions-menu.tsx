/**
 * 表情长按操作菜单（官方同款：长按表情 → 「添加到表情」/「删除表情」）。
 *
 * 官方形态：面板内长按弹底部操作条
 * （`BottomActionDialog`，单项「添加到表情」）；长按「我的表情」里的表情
 * 弹「删除表情」；评论区里长按别人发的图片弹气泡菜单。本仓统一成就地小
 * 菜单——位置贴近官方气泡形态，也不打断输入框焦点。
 */
import { t } from '@/locales';

export function EmoticonActionsMenu({
  x,
  y,
  canRemove,
  onAdd,
  onRemove,
  onClose,
}: {
  /** 视口坐标（长按落点）。 */
  x: number;
  y: number;
  /** 已在「我的表情」里 → 给「删除表情」而不是「添加到表情」。 */
  canRemove: boolean;
  onAdd: () => void;
  onRemove: () => void;
  onClose: () => void;
}) {
  return (
    <>
      {/* 透明遮罩：点外面收起（官方气泡同样点外面 dismiss） */}
      <div
        className="fixed inset-0 z-[60]"
        onClick={onClose}
        onContextMenu={(e) => e.preventDefault()}
      />
      <div
        className="fixed z-[60] flex flex-col overflow-hidden rounded-lg border border-white/10 bg-black/90 py-1 shadow-lg backdrop-blur-sm"
        // 菜单落在指针上方（官方气泡的箭头朝下指表情）
        style={{ left: x, top: y, transform: 'translate(-50%, -110%)' }}
        onContextMenu={(e) => e.preventDefault()}
      >
        {canRemove ? (
          <button
            type="button"
            onClick={onRemove}
            className="cursor-pointer px-3 py-1.5 text-left text-xs whitespace-nowrap text-white/90 transition-colors hover:bg-white/10"
          >
            {t('player.emoticon.removeFromMine')}
          </button>
        ) : (
          <button
            type="button"
            onClick={onAdd}
            className="cursor-pointer px-3 py-1.5 text-left text-xs whitespace-nowrap text-white/90 transition-colors hover:bg-white/10"
          >
            {t('player.emoticon.addToMine')}
          </button>
        )}
      </div>
    </>
  );
}
