import { createFileRoute } from '@tanstack/react-router';
import { DownloadPage } from '@/features/series/components/download-page';

export const Route = createFileRoute('/download')({
  component: DownloadPage,
});