import { Button } from '@/components/ui/button';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { t, tf } from '@/i18n';
import { useUpdate } from './update-context';

/** 设置页的更新卡片（对齐 hgplayer 的 about 卡）：标题 + 版本行 + 右侧
 *  三态按钮（检查更新 / 检查中… / 查看新版本）。 */
export function UpdateCard() {
  const { phase, version, checked, available, setDialogOpen, checkForUpdate } = useUpdate();
  const busy = phase === 'checking';
  // 下载失败（error）也还拿着可用更新——回弹层重试，不回检查态
  const hasUpdate = available != null && phase !== 'checking';

  const status = hasUpdate
    ? tf('update.newVersionAvailable', { version: available.version })
    : checked
      ? t('update.upToDate')
      : '';

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-3">
          <CardTitle className="text-base">{t('update.appTitle')}</CardTitle>
          {hasUpdate ? (
            <Button size="sm" className="ml-auto" onClick={() => setDialogOpen(true)}>
              {t('update.viewNewVersion')}
            </Button>
          ) : (
            <Button
              size="sm"
              variant="outline"
              className="ml-auto"
              disabled={busy}
              onClick={() => void checkForUpdate(true)}
            >
              {busy ? t('update.checking') : t('update.checkUpdate')}
            </Button>
          )}
        </div>
        {version && (
          <CardDescription className="tabular-nums">
            {tf('update.versionLine', { version })}
            {status && ` · ${status}`}
          </CardDescription>
        )}
      </CardHeader>
    </Card>
  );
}
