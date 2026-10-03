import { createFileRoute } from '@tanstack/react-router';
import { RankPage } from '@/features/rank/components/rank-page';

export const Route = createFileRoute('/rank')({
  component: RankPage,
});
