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
