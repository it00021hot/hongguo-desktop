import { createFileRoute } from '@tanstack/react-router';
import { ReservationPage } from '@/pages/reservation/components/reservation-page';

export const Route = createFileRoute('/reservations')({
  component: ReservationPage,
});
