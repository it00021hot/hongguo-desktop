import { createFileRoute } from '@tanstack/react-router';
import { TasksPage } from '@/features/download/components/tasks-page';

export const Route = createFileRoute('/tasks')({
  component: TasksPage,
});
