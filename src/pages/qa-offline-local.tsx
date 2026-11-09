import { createFileRoute, Navigate } from '@tanstack/react-router';
import { offlineQa } from '@/utils/offline-qa';
import { OfflineQaLocalPlayPanel } from '@/features/offline-qa/local-play-panel';

function QaOfflineLocalPage() {
  if (!offlineQa()) {
    return <Navigate to="/" replace />;
  }
  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-4 p-6">
      <h1 className="text-lg font-medium">离线 QA · 本地样本</h1>
      <OfflineQaLocalPlayPanel />
    </div>
  );
}

export const Route = createFileRoute('/qa-offline-local')({
  component: QaOfflineLocalPage,
});
