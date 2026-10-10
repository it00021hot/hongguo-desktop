import { createFileRoute } from '@tanstack/react-router';
import { NewDramaPage } from '@/pages/new-drama/components/new-drama-page';

export const Route = createFileRoute('/new')({
  component: NewDramaPage,
});
