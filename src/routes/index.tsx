import { createFileRoute } from '@tanstack/react-router';
import { HomePage } from '@/features/feed/components/home-page';

export const Route = createFileRoute('/')({
  component: HomePage,
});
