import { Loader2 } from 'lucide-react';
import { t } from '@/i18n';

/** 底部悬浮的「正在解析剧集」提示。首页信息流与浏览页共用。 */
export function ResolvingPill() {
  return (
    <p className="text-muted-foreground bg-card fixed bottom-4 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded-full border px-4 py-2 text-sm shadow-lg">
      <Loader2 className="size-4 animate-spin" aria-hidden />
      {t('common.resolving')}
    </p>
  );
}
