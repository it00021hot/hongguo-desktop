/** 设置页的开关行：Switch 与文本 Label 同 id 联动，点文字即可切换。 */
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';

export function ToggleRow({
  id,
  label,
  checked,
  onChange,
}: {
  id: string;
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center gap-3">
      <Switch id={id} checked={checked} onCheckedChange={onChange} />
      <Label htmlFor={id} className="cursor-pointer">
        {label}
      </Label>
    </div>
  );
}
