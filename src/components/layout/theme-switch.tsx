import { Moon, Sun, Languages } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useThemeStore, type Theme } from '@/lib/stores/mod';
import { useLocaleStore } from '@/lib/stores/locale';
import { t } from '@/i18n';

const THEMES: { value: Theme; labelKey: string }[] = [
  { value: 'auto', labelKey: 'settings.themeAuto' },
  { value: 'light', labelKey: 'settings.themeLight' },
  { value: 'dark', labelKey: 'settings.themeDark' },
];

export function ThemeSwitch() {
  const theme = useThemeStore((s) => s.theme);
  const setTheme = useThemeStore((s) => s.setTheme);
  const resolved = useThemeStore((s) => s.resolved());
  const localeValue = useLocaleStore((s) => s.locale);
  const setLocaleValue = useLocaleStore((s) => s.set);

  return (
    <div className="flex items-center gap-1">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon" aria-label="语言 / Language">
            <Languages className="size-4" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem onSelect={() => setLocaleValue('zh-CN')}>
            简体中文
            {localeValue === 'zh-CN' && <span className="ml-auto text-xs">✓</span>}
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={() => setLocaleValue('en-US')}>
            English
            {localeValue === 'en-US' && <span className="ml-auto text-xs">✓</span>}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon" aria-label={t('common.toggleTheme')}>
            {resolved === 'dark' ? <Moon className="size-4" /> : <Sun className="size-4" />}
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {THEMES.map((item) => (
            <DropdownMenuItem key={item.value} onSelect={() => setTheme(item.value)}>
              {t(item.labelKey)}
              {theme === item.value && <span className="ml-auto text-xs">✓</span>}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
