import { createFileRoute } from '@tanstack/react-router';
import { MergePage } from '@/features/merge/components/merge-page';

export const Route = createFileRoute('/merge')({
  component: MergePage,
});