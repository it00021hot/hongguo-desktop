import { createFileRoute } from '@tanstack/react-router';
import { NewDramaPage } from '@/features/new-drama/components/new-drama-page';

export const Route = createFileRoute('/new')({
  component: NewDramaPage,
});
