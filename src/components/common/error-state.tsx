import type { ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty';
import { t } from '@/locales';

/**
 * 查询失败的统一重试块：主文案 + 可选错误详情 + 重试按钮/自定义动作。
 *
 * 豁免区外列表与页面的错误态都走这里（player 区红色皮肤不在此列）。
 * 各页手写块的常转 Loader2 是非 pending 态的装饰误导，收编后不再保留。
 */
export function ErrorState({
  message,
  detail,
  onRetry,
  icon,
  children,
}: {
  message: string;
  /** 原始错误信息：只服务排查，弱化展示 */
  detail?: string;
  onRetry?: () => void;
  /** 可选图标（如「未登录」态的 Star） */
  icon?: ReactNode;
  /** 追加动作区（如详情页的 ResolveButton 这类专属重试件） */
  children?: ReactNode;
}) {
  return (
    <Empty className="py-16">
      <EmptyHeader>
        {icon !== undefined && <EmptyMedia>{icon}</EmptyMedia>}
        <EmptyTitle className="text-muted-foreground text-sm font-normal">{message}</EmptyTitle>
        {detail !== undefined && detail !== '' && (
          <EmptyDescription className="text-destructive text-xs">{detail}</EmptyDescription>
        )}
      </EmptyHeader>
      {(onRetry !== undefined || children !== undefined) && (
        <EmptyContent>
          {children}
          {onRetry !== undefined && (
            <Button variant="outline" size="sm" onClick={onRetry}>
              {t('feed.retry')}
            </Button>
          )}
        </EmptyContent>
      )}
    </Empty>
  );
}
