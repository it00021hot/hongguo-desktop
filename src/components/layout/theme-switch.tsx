import { Moon, Sun, Languages } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useThemeStore, type Theme } from '@/stores/theme';
import { useLocaleStore } from '@/stores/locale';
import { t, type MessageKey } from '@/locales';

const THEMES: { value: Theme; labelKey: MessageKey }[] = [
  { value: 'auto', labelKey: 'settings.themeAuto' },
  { value: 'light', labelKey: 'settings.themeLight' },
  { value: 'dark', labelKey: 'settings.themeDark' },
];

const LANGUAGES = [
  { value: 'zh-CN', labelKey: 'search.langZh' },
  { value: 'en-US', labelKey: 'search.langEn' },
] as const;

export function ThemeSwitch() {
  const theme = useThemeStore((s) => s.theme);
  const setTheme = useThemeStore((s) => s.setTheme);
  const resolved = useThemeStore((s) => s.resolved());
  const localeValue = useLocaleStore((s) => s.locale);
  const setLocaleValue = useLocaleStore((s) => s.set);

  return (
    <div className="flex h-full items-center gap-1">
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="icon"
            aria-label={t('search.langLabel')}
            className="size-7"
          >
            <Languages className="size-4" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {LANGUAGES.map((item) => (
            <DropdownMenuItem key={item.value} onSelect={() => setLocaleValue(item.value)}>
              {t(item.labelKey)}
              {localeValue === item.value && <span className="ml-auto text-xs">✓</span>}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>

      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="icon"
            aria-label={t('common.toggleTheme')}
            className="size-7"
          >
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
