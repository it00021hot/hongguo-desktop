import { useEffect, useRef, useState } from 'react';
import { Loader2, Send, ShieldCheck, X } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { login as loginApi } from '@/service/commands';
import { useEvent } from '@/service/tauri/events';
import { useAuthRefresh } from '@/service/queries';
import { t, tf } from '@/i18n';
import type { LoginResult } from '@/service/schema';

/**
 * 登录弹窗（hgplayer 同款形态）：表单 → MFA 上行短信等待 → 自动登录。
 *
 * MFA 轮询在 **Rust 后台**（3s 一次，回复短信后自动登录并发
 * `login-mfa-state` 事件）——弹窗关掉也不丢状态，重新打开仍能看到
 * 等待中的验证；成功事件到达时若弹窗已关，用 toast 提示。
 *
 * 登录成功统一在这里失效账号相关缓存（useAuthRefresh）：互动栏/书架/
 * 预约立刻翻面，不等 staleTime。
 */
export function LoginDialog({
  open,
  onOpenChange,
  onSuccess,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSuccess: () => void;
}) {
  const refreshAuth = useAuthRefresh();
  const [mobile, setMobile] = useState('');
  const [code, setCode] = useState('');
  // 发码倒计时按手机号维度：切号立即可发、切回恢复剩余时间
  const [codeDeadlines, setCodeDeadlines] = useState<Record<string, number>>({});
  const [now, setNow] = useState(() => Date.now());
  const [busy, setBusy] = useState(false);
  // mfa 非空 = 上行短信等待中（tips 显示回复方式）
  const [mfaTips, setMfaTips] = useState<string | null>(null);
  const codeInputRef = useRef<HTMLInputElement | null>(null);

  const countdown = Math.max(0, Math.ceil(((codeDeadlines[mobile] ?? 0) - now) / 1000));
  useEffect(() => {
    if (countdown <= 0) return;
    const id = setTimeout(() => setNow(Date.now()), 1000);
    return () => clearTimeout(id);
  }, [countdown]);

  // 后台 MFA 轮询事件：成功（自动登录完成）/ 失败 / 新一轮验证提示
  useEvent<{ state: string; message?: string; name?: string }>('login-mfa-state', (payload) => {
    if (payload.state === 'success') {
      setMfaTips(null);
      setCode('');
      refreshAuth();
      toast.success(tf('settings.loginSuccess', { name: payload.name ?? '' }));
      onOpenChange(false);
      onSuccess();
    } else if (payload.state === 'failed') {
      setMfaTips(null);
      toast.error(payload.message ?? t('settings.loginMfaFailed'));
    } else if (payload.state === 'waiting' && payload.message) {
      setMfaTips(payload.message);
    }
  });

  useEffect(() => {
    if (!open) return;
    // 重开弹窗时若后台仍在 MFA 等待，恢复提示（后台事件 waiting 只推一次）
    void loginApi
      .mfaVerify()
      .then((r) => {
        if (r.kind === 'mfa') setMfaTips(r.tips);
      })
      .catch(() => {
        /* 无进行中的验证，正常 */
      });
  }, [open]);

  const sendCode = async () => {
    if (busy || countdown > 0) return;
    setBusy(true);
    try {
      const out = await loginApi.sendCode(mobile.trim());
      const wait = out.retryTime || 60;
      toast.success(tf('settings.codeSent', { s: wait }));
      setCodeDeadlines((prev) => ({ ...prev, [mobile.trim()]: Date.now() + wait * 1000 }));
      setNow(Date.now());
      codeInputRef.current?.focus();
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
      const r: LoginResult = await loginApi.smsLogin(mobile.trim(), code.trim());
      if (r.kind === 'success') {
        setCode('');
        refreshAuth();
        toast.success(tf('settings.loginSuccess', { name: r.user.name || r.user.userId }));
        onOpenChange(false);
        onSuccess();
      } else if (r.kind === 'mfa') {
        setMfaTips(r.tips);
        toast.info(t('settings.loginMfaTips'));
      }
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const cancelMfa = async () => {
    await loginApi.mfaCancel().catch(() => {});
    setMfaTips(null);
  };

  if (!open) return null;

  const maskedMobile =
    mobile.length === 11 ? `${mobile.slice(0, 3)}****${mobile.slice(7)}` : mobile;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      onClick={(e) => e.target === e.currentTarget && onOpenChange(false)}
    >
      <div className="bg-background w-full max-w-sm rounded-xl border shadow-2xl">
        <div className="flex items-center justify-between border-b px-5 py-4">
          <h2 className="text-base font-semibold">{t('settings.loginTitle')}</h2>
          <button
            type="button"
            className="text-muted-foreground hover:text-foreground"
            onClick={() => onOpenChange(false)}
            aria-label="close"
          >
            <X className="size-4" />
          </button>
        </div>

        <div className="flex flex-col gap-3 px-5 py-4">
          {mfaTips ? (
            <>
              <div className="flex items-center gap-2 text-sm font-medium">
                <Loader2 className="size-4 animate-spin" />
                {t('settings.loginMfaWaiting')}
              </div>
              <p className="text-muted-foreground text-sm">{maskedMobile}</p>
              <div className="bg-muted flex items-start gap-2 rounded-lg p-3 text-sm">
                <ShieldCheck className="text-primary mt-0.5 size-4 shrink-0" />
                <span className="font-medium">{mfaTips}</span>
              </div>
              <p className="text-muted-foreground text-xs">{t('settings.loginMfaBackground')}</p>
              <Button variant="outline" onClick={() => void cancelMfa()}>
                {t('common.cancel')}
              </Button>
            </>
          ) : (
            <>
              <Input
                inputMode="numeric"
                placeholder={t('settings.loginMobilePlaceholder')}
                value={mobile}
                maxLength={11}
                onChange={(e) => setMobile(e.target.value.replace(/\D/g, ''))}
              />
              <div className="flex gap-2">
                <Input
                  ref={codeInputRef}
                  inputMode="numeric"
                  placeholder={t('settings.loginCodePlaceholder')}
                  value={code}
                  maxLength={6}
                  className="flex-1"
                  onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))}
                  onKeyDown={(e) => {
                    // 输入法组合中的 Enter 是字母上屏，不是提交
                    if (e.nativeEvent.isComposing || e.nativeEvent.keyCode === 229) return;
                    if (e.key === 'Enter') void doLogin();
                  }}
                />
                <Button
                  variant="outline"
                  disabled={busy || countdown > 0 || mobile.length !== 11}
                  onClick={() => void sendCode()}
                >
                  <Send className="size-4" />
                  {countdown > 0
                    ? tf('settings.loginResend', { s: countdown })
                    : t(busy ? 'settings.loginSending' : 'settings.loginSendCode')}
                </Button>
              </div>
              <Button
                disabled={busy || code.length < 4 || mobile.length !== 11}
                onClick={() => void doLogin()}
              >
                {busy ? t('settings.loginBusy') : t('settings.loginAction')}
              </Button>
              <p className="text-muted-foreground text-xs">{t('settings.loginRiskNote')}</p>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
