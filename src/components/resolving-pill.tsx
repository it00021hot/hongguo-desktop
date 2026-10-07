import { useEffect, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { t } from '@/i18n';

/**
 * 底部悬浮的「正在解析剧集」提示，全局多处共用。
 *
 * 延迟 300ms 才出现：缓存命中的瞬时解析不闪气泡，慢请求才提示——
 * 提示本身也是一种视觉噪音，能不出现就不出现。
 */
export function ResolvingPill() {
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setVisible(true), 300);
    return () => clearTimeout(timer);
  }, []);
  if (!visible) return null;
  return (
    <p className="text-muted-foreground bg-card fixed bottom-4 left-1/2 z-50 flex -translate-x-1/2 items-center gap-2 rounded-full border px-4 py-2 text-sm shadow-lg">
      <Loader2 className="size-4 animate-spin" aria-hidden />
      {t('common.resolving')}
    </p>
  );
}
