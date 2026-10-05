import { createFileRoute } from '@tanstack/react-router';
import { HistoryPage } from '@/features/history/components/history-page';

export const Route = createFileRoute('/history')({
  component: HistoryPage,
});
