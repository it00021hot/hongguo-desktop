import { Toaster as Sonner, type ToasterProps } from 'sonner';
import { useThemeStore } from '@/stores/theme';

function Toaster({ ...props }: ToasterProps) {
  const resolved = useThemeStore((s) => s.resolved());
  return (
    <Sonner
      theme={resolved}
      className="toaster group"
      position="bottom-right"
      // 默认 4s 太黏：成功提示读完就没人看了，还挡住右下角内容。
      // 真正需要用户停下来读的界面错误不靠 toast 承载。
      duration={2500}
      toastOptions={{
        classNames: {
          toast:
            'group toast group-[.toaster]:bg-popover group-[.toaster]:text-popover-foreground group-[.toaster]:border-border group-[.toaster]:shadow-lg group-[.toaster]:rounded-lg',
          description: 'group-[.toast]:text-muted-foreground',
        },
      }}
      style={
        {
          '--normal-bg': 'var(--popover)',
          '--normal-text': 'var(--popover-foreground)',
          '--normal-border': 'var(--border)',
        } as React.CSSProperties
      }
      {...props}
    />
  );
}

export { Toaster };
