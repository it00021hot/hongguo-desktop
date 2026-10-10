import { createFileRoute } from '@tanstack/react-router';
import { LikedPage } from '@/pages/liked/components/liked-page';

export const Route = createFileRoute('/liked')({
  component: LikedPage,
});
