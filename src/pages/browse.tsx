import { createFileRoute } from '@tanstack/react-router';
import { BrowsePage } from '@/pages/series/components/browse-page';

export const Route = createFileRoute('/browse')({
  component: BrowsePage,
});
