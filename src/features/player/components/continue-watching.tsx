import { useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { History, Play, Trash2, X } from 'lucide-react';
import { toast } from 'sonner';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import {
  useClearPlaybackHistory,
  usePlaybackHistory,
  useRemovePlaybackRecord,
} from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { formatDuration } from '@/lib/format';
import { t, tf } from '@/i18n';

/**
 * 「继续观看」——播放历史。
 *
 * 没有在播任何一集时占据主区域。放在播放器侧栏是不够的：
 * 侧栏只在已经选中剧集时才渲染，而用户找历史恰恰是在「还没在播」的时候，
 * 那样入口等于不存在。
 */
export function ContinueWatching() {
  const navigate = useNavigate();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const { data: history } = usePlaybackHistory();
  const clearHistory = useClearPlaybackHistory();
  const removeRecord = useRemovePlaybackRecord();
  const [confirmClear, setConfirmClear] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState<{ seriesId: string; title: string } | null>(
    null,
  );

  const handleRemove = () => {
    if (!confirmRemove) return;
    removeRecord.mutate(confirmRemove.seriesId, {
      onSuccess: () => {
        toast.success(tf('player.removedRecord', { title: confirmRemove.title }));
        setConfirmRemove(null);
      },
      onError: (e) => toast.error(e.message),
    });
  };

  // 剧名和封面后端已经带在 history 里，不再和剧集列表做关联：
  // 之前靠 useSeriesList 关联，而那个列表过滤了 dismissed，于是用户从磁盘清理页
  // 移除一部剧，它的观看记录就跟着整条消失了 —— 明明数据还在。
  const items = history ?? [];

  if (items.length === 0) {
    return (
      <div className="grid h-full place-items-center p-6">
        <div className="text-center">
          <History className="text-muted-foreground mx-auto size-8" />
          <p className="text-muted-foreground mt-3 text-sm">{t('player.historyEmpty')}</p>
          <p className="text-muted-foreground mt-1 text-xs">{t('player.historyHint')}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4 p-6">
      {/* 清空入口放在标题行：卡片本身是 <button>，按钮里再套按钮是非法 HTML */}
      <div className="flex items-center gap-2">
        <h2 className="flex items-center gap-2 text-sm font-semibold">
          <History className="size-4" />
          {t('player.continueWatching')}
        </h2>
        <Button
          size="sm"
          variant="ghost"
          className="text-destructive ml-auto"
          onClick={() => setConfirmClear(true)}
        >
          <Trash2 className="size-4" />
          {t('player.clearHistory')}
        </Button>
      </div>

      <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 2xl:grid-cols-8">
        {items.map((item) => (
          // 卡片本身不是 button：button 里套 button 是非法 HTML，而「点卡片播放」
          // 和「单条清除」必须是两个独立可点区。改成 article + 整卡播放按钮，
          // 删除按钮叠在它上面（z-10）并阻止事件冒泡，免得删记录时顺手跳播放器。
          <article
            key={item.seriesId}
            className="group bg-card hover:border-foreground/30 relative overflow-hidden rounded-lg border transition-colors"
          >
            <button
              type="button"
              className="block w-full text-left"
              onClick={() => {
                setTarget(item.seriesId, item.vidIndex);
                void navigate({ to: '/player' });
              }}
            >
              <div className="bg-muted relative aspect-3/4 w-full overflow-hidden">
                {item.cover ? (
                  <img src={item.cover} alt="" loading="lazy" className="size-full object-cover" />
                ) : null}
                <span className="bg-background/80 absolute right-2 bottom-2 grid size-8 place-items-center rounded-full opacity-0 transition-opacity group-hover:opacity-100">
                  <Play className="size-4" />
                </span>
              </div>
              <div className="grid gap-1 p-2">
                <p className="truncate text-sm font-medium">{item.title}</p>
                <div className="flex items-center gap-1.5">
                  <Badge variant="secondary" className="text-[10px]">
                    {tf('player.epShort', { index: item.vidIndex })}
                  </Badge>
                  <span className="text-muted-foreground text-xs tabular-nums">
                    {formatDuration(item.currentTime)}
                  </span>
                </div>
              </div>
            </button>

            {/* 不要 Button 组件的任何底色：它会在封面上压出一块方斑，比图标本身还抢眼。
                裸 button + 白色图标，深色投影保证在浅色封面上也看得见。
                点击区仍然留 28px，太小的靶子不好点。 */}
            <button
              type="button"
              className="absolute top-1 right-1 z-10 grid size-7 place-items-center rounded-full text-white opacity-0 drop-shadow-sm transition-opacity group-hover:opacity-100 focus-visible:opacity-100 focus-visible:outline-none"
              aria-label={tf('player.removeRecord', { title: item.title })}
              title={t('player.removeRecordShort')}
              onClick={() => setConfirmRemove({ seriesId: item.seriesId, title: item.title })}
            >
              <X className="size-4" />
            </button>
          </article>
        ))}
      </div>

      <AlertDialog open={confirmClear} onOpenChange={setConfirmClear}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('player.clearHistoryConfirm')}</AlertDialogTitle>
            <AlertDialogDescription>{t('player.clearHistoryDesc')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction onClick={() => clearHistory.mutate()}>
              {t('common.confirm')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={confirmRemove !== null} onOpenChange={(o) => !o && setConfirmRemove(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {confirmRemove && tf('player.removeRecordConfirm', { title: confirmRemove.title })}
            </AlertDialogTitle>
            <AlertDialogDescription>{t('player.removeRecordDesc')}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction onClick={handleRemove}>
              {t('player.removeRecordShort')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
