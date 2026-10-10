import { useState } from 'react';
import { LogOut, LogIn, UserRound } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { login as loginApi } from '@/service/commands';
import { t } from '@/locales';
import type { AccountState } from '@/service/schema';
import { LoginDialog } from './login-dialog';

/**
 * 从 user_info 原文（rawProfile JSON）提取红果号（biz_user_id）。
 * 手机 App「我的」页展示的账号就是这个值——它不在登录响应里，
 * 只有 user_info 下发（2026-10-10 实测）。原文缺失（旧账号未刷新
 * 资料）时返回空串，卡片不显示该行。
 */
function hongguoId(account: AccountState): string {
  if (!account.rawProfile) return '';
  try {
    const v = JSON.parse(account.rawProfile) as { data?: { biz_user_id?: number | string } };
    const id = v?.data?.biz_user_id;
    return id == null ? '' : String(id);
  } catch {
    return '';
  }
}

/**
 * 账户卡片：登录态展示 + 登录弹窗入口。
 *
 * 登录流程（含 MFA 上行短信等待）在 LoginDialog / Rust 后台轮询里，
 * 这里只负责展示账号与退出。
 */
export function AccountCard({
  account,
  onChanged,
}: {
  account: AccountState | null;
  onChanged: () => void;
}) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [logoutConfirm, setLogoutConfirm] = useState(false);

  const logout = async () => {
    try {
      await loginApi.logout();
      toast.success(t('settings.loggedOut'));
      setLogoutConfirm(false);
      onChanged();
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <UserRound className="size-4" />
          {t('settings.account')}
        </CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {account ? (
          <div className="flex flex-wrap items-center gap-3">
            {account.avatarUrl && (
              <img src={account.avatarUrl} alt="" className="size-8 rounded-full object-cover" />
            )}
            <Badge variant="secondary">{account.userName || account.userId}</Badge>
            {hongguoId(account) && (
              <span className="text-muted-foreground text-xs">
                {t('settings.hongguoId')}: <span className="font-mono">{hongguoId(account)}</span>
              </span>
            )}
            <span className="text-muted-foreground font-mono text-xs">
              {account.mobile.slice(0, 3)}****{account.mobile.slice(7)}
            </span>
            <div className="ml-auto flex gap-2">
              <Button variant="outline" size="sm" onClick={() => setLogoutConfirm(true)}>
                <LogOut className="size-4" />
                {t('settings.logout')}
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex items-center justify-between gap-3">
            <p className="text-muted-foreground text-sm">{t('settings.loginPrompt')}</p>
            <Button size="sm" onClick={() => setDialogOpen(true)}>
              <LogIn className="size-4" />
              {t('settings.loginAction')}
            </Button>
          </div>
        )}
        <ConfirmDialog
          open={logoutConfirm}
          onOpenChange={setLogoutConfirm}
          title={t('nav.logoutConfirmTitle')}
          description={t('nav.logoutConfirmBody')}
          confirmLabel={t('settings.logout')}
          onConfirm={() => void logout()}
        />
        <LoginDialog open={dialogOpen} onOpenChange={setDialogOpen} onSuccess={onChanged} />
      </CardContent>
    </Card>
  );
}
