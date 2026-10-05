import { createFileRoute } from '@tanstack/react-router';
import { ReservationPage } from '@/features/reservation/components/reservation-page';

export const Route = createFileRoute('/reservations')({
  component: ReservationPage,
});
