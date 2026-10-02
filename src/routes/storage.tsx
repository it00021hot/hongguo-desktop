import { createFileRoute } from '@tanstack/react-router';
import { StoragePage } from '@/features/storage/components/storage-page';

export const Route = createFileRoute('/storage')({
  component: StoragePage,
});
