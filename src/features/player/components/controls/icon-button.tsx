/** 控制栏图标按钮：ghost 变体统一覆盖为透明底白字，控制栏内按钮共用外观。 */
import { Button } from '@/components/ui/button';

export function IconButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <Button
      variant="ghost"
      size="icon"
      className="size-8 bg-transparent text-white hover:bg-white/20 hover:text-white disabled:opacity-40 disabled:hover:bg-transparent"
      onClick={onClick}
      disabled={disabled}
      title={label}
      aria-label={label}
    >
      {children}
    </Button>
  );
}
