#ifndef HONGGUO_AUDIO_OUTPUT_H
#define HONGGUO_AUDIO_OUTPUT_H
#include <stdint.h>

typedef struct {
  uint32_t sample_rate, channels;
} HgFormat;
typedef struct {
  void *context;
  uint32_t (*read)(void *, float *, uint32_t, uint32_t, uint64_t);
  void (*played)(void *, uint64_t, uint64_t);
} HgCallbacks;
typedef struct {
  uint64_t epoch, device_frames;
  uint32_t latency_frames, reserved;
} HgPosition;
_Static_assert(sizeof(HgFormat) == 8, "PCM format ABI");
_Static_assert(sizeof(HgPosition) == 24, "PCM position ABI");
enum { HG_START = 1, HG_PAUSE, HG_FLUSH };
enum {
  HG_INVALID = -10001,
  HG_NO_MEMORY = -10002,
  HG_FORMAT = -10003,
  HG_DEVICE = -10004
};

int32_t hg_pcm_output_open(HgFormat format, HgCallbacks callbacks,
                           void **output);
int32_t hg_pcm_output_control(void *output, int32_t operation, uint64_t epoch);
int32_t hg_pcm_output_position(void *output, HgPosition *position);
void hg_pcm_output_close(void *output);
#endif
