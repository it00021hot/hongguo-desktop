import { createFileRoute } from '@tanstack/react-router';
import { BrowsePage } from '@/features/series/components/browse-page';

export const Route = createFileRoute('/')({
  component: BrowsePage,
});