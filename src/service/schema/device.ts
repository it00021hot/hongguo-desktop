import { z } from 'zod';

export const registrationStatusSchema = z.object({
  phase: z.enum(['idle', 'registering', 'ready', 'retryScheduled', 'blocked', 'failed']),
  attempt: z.number().int().nonnegative(),
  nextRetryAt: z.number().int().nonnegative().nullable(),
  errorKind: z.string().nullable(),
  errorMessage: z.string().nullable(),
  blockUntil: z.number().int().nonnegative().nullable(),
  isManualBlock: z.boolean(),
});

export const deviceStatusSchema = z.object({
  iid: z.string(),
  deviceId: z.string(),
  source: z.string(),
  previousIid: z.string().nullable(),
  backupIid: z.string().nullable(),
  lastRegisterAttemptMs: z.number().int().nonnegative().nullable(),
  lastRegisterError: z.string().nullable(),
  registration: registrationStatusSchema,
  isPlaybackReady: z.boolean(),
});

export type DeviceStatus = z.infer<typeof deviceStatusSchema>;
