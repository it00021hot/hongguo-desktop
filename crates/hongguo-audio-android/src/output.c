#include "output.h"
#include <aaudio/AAudio.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <time.h>

typedef struct {
  AAudioStream *stream;
  HgFormat format;
  HgCallbacks callbacks;
  atomic_uint_fast64_t epoch, origin, played;
  atomic_int error;
} Output;

static void update_position(Output *output) {
  int64_t frame = 0, time = 0;
  if (AAudioStream_getTimestamp(output->stream, CLOCK_MONOTONIC, &frame,
                                &time) != AAUDIO_OK)
    return;
  uint64_t origin = atomic_load(&output->origin);
  uint64_t current = frame > (int64_t)origin ? (uint64_t)frame - origin : 0;
  uint64_t previous = atomic_load(&output->played);
  while (current > previous &&
         !atomic_compare_exchange_weak(&output->played, &previous, current)) {
  }
  if (current < previous)
    current = previous;
  output->callbacks.played(output->callbacks.context,
                           atomic_load(&output->epoch), current);
}

static aaudio_data_callback_result_t render(AAudioStream *stream, void *context,
                                            void *data, int32_t frames) {
  (void)stream;
  Output *output = context;
  if (frames <= 0)
    return AAUDIO_CALLBACK_RESULT_STOP;
  update_position(output);
  output->callbacks.read(output->callbacks.context, data, frames,
                         output->format.channels, atomic_load(&output->epoch));
  return AAUDIO_CALLBACK_RESULT_CONTINUE;
}

static void failed(AAudioStream *stream, void *context, aaudio_result_t error) {
  (void)stream;
  Output *output = context;
  atomic_store(&output->error, error);
}

int32_t hg_pcm_output_open(HgFormat format, HgCallbacks callbacks,
                           void **result) {
  if (!result || !format.sample_rate || !format.channels || !callbacks.read ||
      !callbacks.played)
    return HG_INVALID;
  *result = NULL;
  Output *output = calloc(1, sizeof(Output));
  if (!output)
    return HG_NO_MEMORY;
  output->format = format;
  output->callbacks = callbacks;
  AAudioStreamBuilder *builder = NULL;
  aaudio_result_t status = AAudio_createStreamBuilder(&builder);
  if (status) {
    free(output);
    return status;
  }
  AAudioStreamBuilder_setDirection(builder, AAUDIO_DIRECTION_OUTPUT);
  AAudioStreamBuilder_setFormat(builder, AAUDIO_FORMAT_PCM_FLOAT);
  AAudioStreamBuilder_setSampleRate(builder, format.sample_rate);
  AAudioStreamBuilder_setChannelCount(builder, format.channels);
  AAudioStreamBuilder_setSharingMode(builder, AAUDIO_SHARING_MODE_SHARED);
  AAudioStreamBuilder_setUsage(builder, AAUDIO_USAGE_MEDIA);
  AAudioStreamBuilder_setContentType(builder, AAUDIO_CONTENT_TYPE_MOVIE);
  AAudioStreamBuilder_setDataCallback(builder, render, output);
  AAudioStreamBuilder_setErrorCallback(builder, failed, output);
  status = AAudioStreamBuilder_openStream(builder, &output->stream);
  AAudioStreamBuilder_delete(builder);
  if (status) {
    free(output);
    return status;
  }
  if (AAudioStream_getFormat(output->stream) != AAUDIO_FORMAT_PCM_FLOAT ||
      AAudioStream_getSampleRate(output->stream) !=
          (int32_t)format.sample_rate ||
      AAudioStream_getChannelCount(output->stream) !=
          (int32_t)format.channels) {
    hg_pcm_output_close(output);
    return HG_FORMAT;
  }
  *result = output;
  return AAUDIO_OK;
}

static aaudio_result_t settle(Output *output, aaudio_stream_state_t wanted) {
  aaudio_stream_state_t current = AAudioStream_getState(output->stream);
  for (unsigned transitions = 0; current != wanted && transitions < 8;
       ++transitions) {
    if (current == AAUDIO_STREAM_STATE_DISCONNECTED)
      return AAUDIO_ERROR_DISCONNECTED;
    aaudio_stream_state_t next = current;
    aaudio_result_t status = AAudioStream_waitForStateChange(
        output->stream, current, &next, 1000000000);
    if (status)
      return status;
    current = next;
  }
  return current == wanted ? AAUDIO_OK : AAUDIO_ERROR_TIMEOUT;
}

int32_t hg_pcm_output_control(void *handle, int32_t operation, uint64_t epoch) {
  Output *output = handle;
  if (!output)
    return HG_INVALID;
  int32_t error = atomic_load(&output->error);
  if (error)
    return error;
  aaudio_stream_state_t state = AAudioStream_getState(output->stream);
  if (operation == HG_START) {
    if (state == AAUDIO_STREAM_STATE_STARTED)
      return AAUDIO_OK;
    aaudio_result_t status = AAudioStream_requestStart(output->stream);
    return status ? status : settle(output, AAUDIO_STREAM_STATE_STARTED);
  }
  if (operation != HG_PAUSE && operation != HG_FLUSH)
    return HG_INVALID;
  update_position(output);
  if (state == AAUDIO_STREAM_STATE_STARTED ||
      state == AAUDIO_STREAM_STATE_STARTING) {
    aaudio_result_t status = AAudioStream_requestPause(output->stream);
    if (status)
      return status;
    status = settle(output, AAUDIO_STREAM_STATE_PAUSED);
    if (status)
      return status;
  }
  if (operation == HG_PAUSE)
    return AAUDIO_OK;
  aaudio_result_t status = AAudioStream_requestFlush(output->stream);
  if (status)
    return status;
  status = settle(output, AAUDIO_STREAM_STATE_FLUSHED);
  if (status)
    return status;
  atomic_store(&output->origin,
               (uint64_t)AAudioStream_getFramesWritten(output->stream));
  atomic_store(&output->played, 0);
  atomic_store(&output->epoch, epoch);
  return AAUDIO_OK;
}

int32_t hg_pcm_output_position(void *handle, HgPosition *position) {
  Output *output = handle;
  if (!output || !position)
    return HG_INVALID;
  int32_t error = atomic_load(&output->error);
  if (error)
    return error;
  update_position(output);
  position->epoch = atomic_load(&output->epoch);
  position->device_frames = atomic_load(&output->played);
  position->latency_frames =
      (uint32_t)AAudioStream_getBufferSizeInFrames(output->stream);
  position->reserved = 0;
  return AAUDIO_OK;
}

void hg_pcm_output_close(void *handle) {
  Output *output = handle;
  if (!output)
    return;
  if (output->stream)
    AAudioStream_close(output->stream);
  free(output);
}
