import { createFileRoute } from '@tanstack/react-router';
import { HomePage } from '@/pages/home/components/home-page';

export const Route = createFileRoute('/')({
  component: HomePage,
});
