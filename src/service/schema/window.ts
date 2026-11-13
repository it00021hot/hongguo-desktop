import { z } from 'zod';

export const windowPresentationSchema = z.object({
  mode: z.enum(['normal', 'mini', 'portrait']),
  isPortraitFull: z.boolean(),
  isFrameless: z.boolean(),
  isPinned: z.boolean(),
  isFullscreen: z.boolean(),
});

export type WindowPresentation = z.infer<typeof windowPresentationSchema>;
