import { createFileRoute } from '@tanstack/react-router';
import { PlayerPage } from '@/features/player/components/player-page';

export const Route = createFileRoute('/player')({
  component: PlayerPage,
});
