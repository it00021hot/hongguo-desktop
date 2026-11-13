import { useEffect, useState } from 'react';
import { Globe } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { t, tf } from '@/locales';
import { cn } from '@/lib/utils';
import { bossKeyConflicts, bossKeyFromEvent, type BossKeyConflict } from '@/utils/boss-key';
import { shortcutKeyLabel, type ShortcutAction } from '@/stores/shortcuts';

/** 老板键 vs 播放器快捷键冲突检测：accelerator 末段裸键 vs 播放器
 *  `e.key`（全局组合在应用内按下会同时触发播放器动作）。 */
export function resolveBossKeyConflicts(
  bossKey: string,
  keys: Record<ShortcutAction, string>,
  spaceLabel: string,
): BossKeyConflict[] {
  const core = bossKey
    .split('+')
    .map((p) => p.trim().toLowerCase())
    .filter(Boolean)
    .pop();
  if (!core) return [];
  const actions: { action: ShortcutAction; label: string; key: string }[] = [
    {
      action: 'playPause',
      label: shortcutKeyLabel(keys.playPause, spaceLabel),
      key: keys.playPause,
    },
    { action: 'seekBack', label: shortcutKeyLabel(keys.seekBack, spaceLabel), key: keys.seekBack },
    {
      action: 'seekForward',
      label: shortcutKeyLabel(keys.seekForward, spaceLabel),
      key: keys.seekForward,
    },
    { action: 'stepPrev', label: shortcutKeyLabel(keys.stepPrev, spaceLabel), key: keys.stepPrev },
    { action: 'stepNext', label: shortcutKeyLabel(keys.stepNext, spaceLabel), key: keys.stepNext },
  ];
  return bossKeyConflicts(bossKey, actions);
}

/** 老板键卡片（全局快捷键，独立于播放器快捷键卡）：点击按钮捕获组合键
 *  （Ctrl/Cmd + 字母/数字），应用外也生效，隐藏窗口并暂停播放。
 *  值走主设置的草稿/保存流，注册在保存后端钩子里。 */
export function BossKeyCard({
  bossKey,
  conflicts,
  onChange,
}: {
  bossKey: string;
  conflicts: BossKeyConflict[];
  onChange: (v: string) => void;
}) {
  const [capturing, setCapturing] = useState(false);

  // 捕获态：窗口级 keydown 抢先（capture 阶段），Esc 取消，纯修饰键忽略
  useEffect(() => {
    if (!capturing) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') {
        setCapturing(false);
        return;
      }
      const accel = bossKeyFromEvent(e);
      if (accel === null) return;
      onChange(accel);
      setCapturing(false);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [capturing, onChange]);

  const display = bossKey
    ? bossKey
        .split('+')
        .map((p) => {
          const s = p.trim();
          if (s === 'CommandOrControl') return 'Ctrl/Cmd';
          return s;
        })
        .join(' + ')
    : t('settings.bossKeyOff');

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <Globe className="size-4" aria-hidden />
          {t('settings.bossKeyCardTitle')}
        </CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-2">
        <div className="flex items-center justify-between">
          <span className="text-sm">{t('settings.bossKey')}</span>
          <Button
            size="sm"
            variant="outline"
            className={cn('min-w-32 font-mono', capturing && 'border-primary text-primary')}
            onClick={() => setCapturing(true)}
          >
            {capturing ? t('settings.bossKeyCapturing') : display}
          </Button>
        </div>
        {conflicts.length > 0 && (
          <p className="text-destructive text-xs">
            {tf('settings.bossKeyConflict', {
              actions: conflicts.map((c) => c.label).join('、'),
            })}
          </p>
        )}
        <p className="text-muted-foreground text-xs">{t('settings.bossKeyDesc')}</p>
        <p className="text-muted-foreground text-xs">{t('settings.bossKeyGlobalHint')}</p>
      </CardContent>
    </Card>
  );
}
