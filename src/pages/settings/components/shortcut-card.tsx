import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { t, tf } from '@/locales';
import { shortcutKeyLabel, useShortcutsStore, type ShortcutAction } from '@/stores/shortcuts';

const ACTIONS: { action: ShortcutAction; labelKey: string }[] = [
  { action: 'playPause', labelKey: 'settings.keyPlayPause' },
  { action: 'seekBack', labelKey: 'settings.keySeekBack' },
  { action: 'seekForward', labelKey: 'settings.keySeekForward' },
  { action: 'stepPrev', labelKey: 'settings.keyStepPrev' },
  { action: 'stepNext', labelKey: 'settings.keyStepNext' },
];

/** 播放器快捷键自定义卡。映射存前端 store（hongguo-shortcuts），
 *  与后端 Settings 的保存按钮无关，点了立即生效。 */
export function ShortcutCard() {
  const keys = useShortcutsStore((s) => s.keys);
  const setKey = useShortcutsStore((s) => s.setKey);
  const reset = useShortcutsStore((s) => s.reset);
  const [capturing, setCapturing] = useState<ShortcutAction | null>(null);

  // 捕获态：窗口级 keydown 抢先（capture 阶段），Esc 取消，纯修饰键忽略
  useEffect(() => {
    if (!capturing) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') {
        setCapturing(null);
        return;
      }
      if (['Shift', 'Control', 'Alt', 'Meta'].includes(e.key)) return;
      const owner = ACTIONS.find(({ action }) => action !== capturing && keys[action] === e.key);
      if (owner) {
        toast.error(
          tf('settings.keyConflict', { key: shortcutKeyLabel(e.key, t('settings.keySpace')) }),
        );
        setCapturing(null);
        return;
      }
      setKey(capturing, e.key);
      setCapturing(null);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [capturing, keys, setKey]);

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-3">
          <CardTitle className="text-base">{t('settings.shortcuts')}</CardTitle>
          <Button size="sm" variant="ghost" className="ml-auto" onClick={() => reset()}>
            {t('settings.resetKeys')}
          </Button>
        </div>
      </CardHeader>
      <CardContent className="flex flex-col gap-1">
        <p className="text-muted-foreground mb-2 text-xs">{t('settings.shortcutsHint')}</p>
        {ACTIONS.map(({ action, labelKey }) => (
          <div key={action} className="flex items-center justify-between py-1">
            <span className="text-sm">{t(labelKey)}</span>
            <Button
              size="sm"
              variant="outline"
              className="min-w-24 font-mono"
              onClick={() => setCapturing(action)}
            >
              {capturing === action
                ? t('settings.keyCapturing')
                : shortcutKeyLabel(keys[action], t('settings.keySpace'))}
            </Button>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}
