import { createFileRoute } from '@tanstack/react-router';
import { HistoryPage } from '@/pages/history/components/history-page';

export const Route = createFileRoute('/history')({
  component: HistoryPage,
});
