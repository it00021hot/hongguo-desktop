import { Toaster as Sonner, type ToasterProps } from 'sonner';
import { useThemeStore } from '@/lib/stores/theme';

function Toaster({ ...props }: ToasterProps) {
  const resolved = useThemeStore((s) => s.resolved());
  return (
    <Sonner
      theme={resolved}
      className="toaster group"
      position="bottom-right"
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
