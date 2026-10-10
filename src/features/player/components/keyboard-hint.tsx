/** 暂停时中部露出的快捷键提示胶囊（显示条件由调用处裁决）。 */
import { t } from '@/locales';

export function KeyboardHint() {
  return (
    <div className="pointer-events-none absolute inset-x-0 top-[38%] z-10 flex justify-center">
      <span className="rounded-full bg-black/55 px-4 py-1.5 text-xs text-white/75 backdrop-blur-sm">
        {t('player.keyboardHint')}
      </span>
    </div>
  );
}
