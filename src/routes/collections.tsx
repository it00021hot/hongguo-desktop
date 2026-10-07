import { createFileRoute } from '@tanstack/react-router';
import { CollectionPage } from '@/features/collection/components/collection-page';

export const Route = createFileRoute('/collections')({
  component: CollectionPage,
});
