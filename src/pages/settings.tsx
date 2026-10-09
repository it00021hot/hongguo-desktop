import { createFileRoute } from '@tanstack/react-router';
import { SettingsPage } from '@/pages/settings/components/settings-page';

export const Route = createFileRoute('/settings')({
  component: SettingsPage,
});
