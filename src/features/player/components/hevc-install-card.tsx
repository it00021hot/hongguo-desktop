/** HEVC 扩展安装引导卡。`floating`：进播放页的 proactive 提示（每会话一次可关闭）；
 *  `blocking`：解码失败后的居中主卡，装扩展或转兼容必须二选一、不装不转不消失（选过转兼容本会话不再问）。
 *  安装走后端命令（下载 → SHA-256 校验 → Add-AppxPackage），成功后刷新能力缓存并由父层重试起播。 */
import { useState } from 'react';
import { Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { t, tf } from '@/locales';
import { hevcSession } from '../hooks/hevc-session';
import { useInstallHevcExtension } from '@/service/queries';

export function HevcInstallCard({
  hevcPlayable,
  variant,
  onInstalled,
  onChooseCompat,
}: {
  hevcPlayable: boolean;
  variant: 'floating' | 'blocking';
  /** 安装成功且能力重探通过后由父层触发（画面没在播时重试起播） */
  onInstalled: () => void;
  /** blocking 专属：用户选择「暂不安装，转兼容格式播放」 */
  onChooseCompat?: () => void;
}) {
  const install = useInstallHevcExtension();
  const [dismissed, setDismissed] = useState(hevcSession.isDismissed());

  if (hevcPlayable || (variant === 'floating' && dismissed)) return null;

  const runInstall = () =>
    install.mutate(undefined, {
      onSuccess: (playable) => {
        if (playable) {
          toast.success(t('player.hevcInstallSuccess'));
          onInstalled();
        } else {
          // 装了却探不到：罕见（版本过老的 WebView2 等），重启兜底
          toast.warning(t('player.hevcStillNoPicture'));
        }
      },
      onError: (e) => {
        toast.error(tf('player.hevcInstallFailed', { reason: e.message }));
      },
    });

  if (variant === 'blocking') {
    return (
      <div className="absolute inset-0 z-30 grid place-items-center bg-black/85 p-6">
        <div className="bg-popover text-popover-foreground flex w-full max-w-sm flex-col items-start gap-2 rounded-lg border p-5 shadow-lg">
          <p className="text-sm font-medium">{t('player.hevcCardTitle')}</p>
          <p className="text-muted-foreground text-xs leading-relaxed">
            {t('player.hevcCardDesc')}
          </p>
          <div className="mt-2 flex w-full flex-col gap-2">
            <Button size="sm" disabled={install.isPending} onClick={runInstall}>
              {install.isPending && <Loader2 className="size-3.5 animate-spin" />}
              {install.isPending ? t('player.hevcInstalling') : t('player.hevcInstall')}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={install.isPending}
              onClick={() => {
                hevcSession.authorizeCompat();
                onChooseCompat?.();
              }}
            >
              {t('player.hevcChooseCompat')}
            </Button>
          </div>
        </div>
      </div>
    );
  }

  return (
    // 底部居中悬浮而不是全屏盖层：不打断正在播的内容
    <div className="absolute inset-x-0 bottom-14 z-30 flex justify-center px-6">
      <div className="bg-popover text-popover-foreground flex w-full max-w-sm flex-col items-start gap-2 rounded-lg border p-4 shadow-lg">
        <p className="text-sm font-medium">{t('player.hevcCardTitle')}</p>
        <p className="text-muted-foreground text-xs leading-relaxed">{t('player.hevcCardDesc')}</p>
        <div className="mt-1 flex w-full gap-2">
          <Button size="sm" className="flex-1" disabled={install.isPending} onClick={runInstall}>
            {install.isPending && <Loader2 className="size-3.5 animate-spin" />}
            {install.isPending ? t('player.hevcInstalling') : t('player.hevcInstall')}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              hevcSession.markDismissed();
              setDismissed(true);
            }}
          >
            {t('player.hevcLater')}
          </Button>
        </div>
      </div>
    </div>
  );
}
