import { useCallback, useEffect, useRef, useState } from 'react';
import { LogOut, RefreshCw, Send, UserRound } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { login as loginApi } from '@/lib/ipc/commands';
import { t, tf } from '@/i18n';
import type { AccountState, LoginResult } from '@/lib/schema';

/**
 * 账户卡片：短信验证码登录（hgplayer 同款链路）。
 *
 * 流程：发码 → 登录；服务端要求 MFA 时转上行短信验证轮询
 * （error_code=1045 = 等待用户回复短信）。登录态落在后端
 * `Settings.account`，业务请求自动附带会话 Cookie。
 */
export function AccountCard({ account, onChanged }: { account: AccountState | null; onChanged: () => void }) {
  const [mobile, setMobile] = useState('');
  const [code, setCode] = useState('');
  const [countdown, setCountdown] = useState(0);
  const [busy, setBusy] = useState(false);
  // MFA 上下文：非空表示当前登录处于「等上行短信」阶段
  const [mfa, setMfa] = useState<{ retryTag: string; smsCodeKey: string; tips: string } | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    if (countdown <= 0) return;
    const id = setTimeout(() => setCountdown((c) => c - 1), 1000);
    return () => clearTimeout(id);
  }, [countdown]);

  // MFA 轮询：3s 一次，直到 success / 报错
  useEffect(() => {
    if (!mfa) {
      if (timerRef.current) clearInterval(timerRef.current);
      timerRef.current = null;
      return;
    }
    timerRef.current = setInterval(() => {
      void loginApi
        .mfaVerify(mfa.retryTag, mfa.smsCodeKey, mobile)
        .then((r) => handleResult(r))
        .catch((e: Error) => {
          stopMfa();
          toast.error(e.message);
        });
    }, 3000);
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
      timerRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mfa]);

  const stopMfa = () => setMfa(null);

  const handleResult = (r: LoginResult) => {
    if (r.kind === 'success') {
      stopMfa();
      setCode('');
      toast.success(tf('settings.loginSuccess', { name: r.user.name || r.user.userId }));
      onChanged();
    } else if (r.kind === 'mfa') {
      setMfa({ retryTag: r.retryTag, smsCodeKey: r.smsCodeKey, tips: r.tips });
      toast.info(t('settings.loginMfaTips'));
    }
    // mfaWaiting：轮询继续，无需处理
  };

  const sendCode = async () => {
    if (busy || countdown > 0) return;
    setBusy(true);
    try {
      const msg = await loginApi.sendCode(mobile.trim());
      toast.success(msg);
      setCountdown(60);
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const doLogin = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const r = await loginApi.smsLogin(mobile.trim(), code.trim(), mfa ?? undefined);
      handleResult(r);
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const logout = async () => {
    try {
      await loginApi.logout();
      toast.success(t('settings.loggedOut'));
      onChanged();
    } catch (e) {
      toast.error((e as Error).message);
    }
  };

  const refreshUser = useCallback(async () => {
    try {
      const u = await loginApi.userInfo();
      toast.success(tf('settings.loginSessionOk', { name: u.name || u.userId }));
      onChanged();
    } catch (e) {
      toast.error((e as Error).message);
    }
  }, [onChanged]);

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <UserRound className="size-4" />
          {t('settings.account')}
        </CardTitle>
        <CardDescription>{t('settings.accountDesc')}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {account ? (
          <div className="flex flex-wrap items-center gap-3">
            <Badge variant="secondary">{account.userName || account.userId}</Badge>
            <span className="text-muted-foreground font-mono text-xs">
              {account.mobile.slice(0, 3)}****{account.mobile.slice(7)}
            </span>
            <div className="ml-auto flex gap-2">
              <Button variant="outline" size="sm" onClick={() => void refreshUser()}>
                <RefreshCw className="size-4" />
                {t('settings.loginRefresh')}
              </Button>
              <Button variant="outline" size="sm" onClick={() => void logout()}>
                <LogOut className="size-4" />
                {t('settings.logout')}
              </Button>
            </div>
          </div>
        ) : mfa ? (
          <div className="flex flex-col gap-2">
            <p className="text-muted-foreground text-sm">
              {t('settings.loginMfaWaiting')} {mfa.tips && `（${mfa.tips}）`}
            </p>
            <div className="flex gap-2">
              <Button variant="outline" size="sm" onClick={stopMfa}>
                {t('common.cancel')}
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            <div className="flex gap-2">
              <Input
                inputMode="numeric"
                placeholder={t('settings.loginMobilePlaceholder')}
                value={mobile}
                maxLength={11}
                onChange={(e) => setMobile(e.target.value.replace(/\D/g, ''))}
                className="max-w-48"
              />
              <Button
                variant="outline"
                disabled={busy || countdown > 0 || mobile.length !== 11}
                onClick={() => void sendCode()}
              >
                <Send className="size-4" />
                {countdown > 0 ? tf('settings.loginResend', { s: countdown }) : t('settings.loginSendCode')}
              </Button>
            </div>
            <div className="flex gap-2">
              <Input
                inputMode="numeric"
                placeholder={t('settings.loginCodePlaceholder')}
                value={code}
                maxLength={6}
                onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))}
                className="max-w-48"
                onKeyDown={(e) => {
                  if (e.key === 'Enter') void doLogin();
                }}
              />
              <Button disabled={busy || code.length < 4 || mobile.length !== 11} onClick={() => void doLogin()}>
                {t('settings.loginAction')}
              </Button>
            </div>
            <p className="text-muted-foreground text-xs">{t('settings.loginRiskNote')}</p>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
