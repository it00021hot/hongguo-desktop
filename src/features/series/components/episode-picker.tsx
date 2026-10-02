import { useMemo, useState } from 'react';
import { Check } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Checkbox } from '@/components/ui/checkbox';
import { Badge } from '@/components/ui/badge';
import { firstN, formatRange, lastN, parseRange } from '@/lib/range';
import { t, tf } from '@/i18n';
import type { Episode } from '@/lib/schema';

interface Props {
  episodes: Episode[];
  selected: number[];
  onChange: (next: number[]) => void;
}

/**
 * 选集器。
 *
 * 三种勾选方式并存：网格逐集点选、快捷区间（前 10 / 前 30 / 后 30）、区间语法输入。
 * 区间语法是现版的核心能力——短剧动辄几百集，手动勾选不现实。
 */
export function EpisodePicker({ episodes, selected, onChange }: Props) {
  const [rangeText, setRangeText] = useState('');
  const [rangeError, setRangeError] = useState(false);

  const total = episodes.length;
  const allIndices = useMemo(() => episodes.map((e) => e.vidIndex), [episodes]);
  const selectedSet = useMemo(() => new Set(selected), [selected]);

  const selectAll = () => onChange(allIndices);
  const clear = () => onChange([]);
  const invert = () => onChange(allIndices.filter((i) => !selectedSet.has(i)));

  const applyRange = (text: string) => {
    setRangeText(text);
    if (!text.trim()) {
      setRangeError(false);
      onChange([]);
      return;
    }
    const parsed = parseRange(text, total);
    // 一个都解析不出来才算格式错误；部分解析成功时不报错，静默采用能解析的部分
    if (parsed.length === 0) {
      setRangeError(true);
      return;
    }
    setRangeError(false);
    onChange(parsed);
  };

  const toggle = (index: number) => {
    if (selectedSet.has(index)) {
      onChange(selected.filter((i) => i !== index));
    } else {
      onChange([...selected, index].sort((a, b) => a - b));
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <Label>{t('download.selectEpisodes')}</Label>
        <Badge variant="secondary">{tf('download.selected', { count: selected.length })}</Badge>
      </div>

      <div className="flex flex-wrap gap-2">
        <Button size="sm" variant="outline" onClick={selectAll}>
          {t('download.selectAll')}
        </Button>
        <Button size="sm" variant="outline" onClick={invert}>
          {t('download.invert')}
        </Button>
        <Button size="sm" variant="outline" onClick={clear}>
          {t('download.clear')}
        </Button>
        <Button size="sm" variant="ghost" onClick={() => onChange(firstN(total, 10))}>
          {t('download.first10')}
        </Button>
        <Button size="sm" variant="ghost" onClick={() => onChange(firstN(total, 30))}>
          {t('download.first30')}
        </Button>
        <Button size="sm" variant="ghost" onClick={() => onChange(lastN(total, 30))}>
          {t('download.last30')}
        </Button>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label htmlFor="range-input">{t('download.rangeLabel')}</Label>
        <div className="flex gap-2">
          <Input
            id="range-input"
            value={rangeText}
            onChange={(e) => applyRange(e.target.value)}
            placeholder={t('download.rangePlaceholder')}
            className="font-mono"
            aria-invalid={rangeError}
          />
          {selected.length > 0 && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setRangeText(formatRange(selected))}
              title={t('common.copy')}
            >
              <Check className="size-4" />
            </Button>
          )}
        </div>
        {rangeError && (
          <p className="text-destructive text-xs">{t('download.rangeInvalid')}</p>
        )}
      </div>

      <div className="scrollbar-thin max-h-64 overflow-y-auto rounded-md border p-2">
        <div className="grid grid-cols-6 gap-1 sm:grid-cols-8 md:grid-cols-10">
          {episodes.map((ep) => {
            const checked = selectedSet.has(ep.vidIndex);
            return (
              <label
                key={ep.vidIndex}
                className="hover:bg-accent flex cursor-pointer items-center gap-1 rounded px-1.5 py-1 text-sm tabular-nums"
              >
                <Checkbox
                  checked={checked}
                  onCheckedChange={() => toggle(ep.vidIndex)}
                  aria-label={`${t('download.selectEpisodes')} ${ep.vidIndex}`}
                />
                <span className="truncate">{ep.vidIndex}</span>
              </label>
            );
          })}
        </div>
      </div>
    </div>
  );
}
