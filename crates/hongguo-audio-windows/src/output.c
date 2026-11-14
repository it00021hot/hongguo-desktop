#define COBJMACROS
#include "output.h"
#include "watcher.h"
#include <Audioclient.h>
#include <avrt.h>
#include <mmdeviceapi.h>
#include <stdbool.h>
#include <stdlib.h>
#include <windows.h>

enum { POSITION_QUERY = 4, CLOSE_OUTPUT = 5 };
typedef struct {
  HgFormat format;
  HgCallbacks callbacks;
  HANDLE thread, command, completed, render;
  CRITICAL_SECTION lock;
  int32_t operation, result, failure;
  uint64_t requested_epoch, epoch, frequency, origin;
  HgPosition position;
  IAudioClient *client;
  IAudioRenderClient *renderer;
  IAudioClock *clock;
  IMMDeviceEnumerator *enumerator;
  HgWatcher *watcher;
  uint32_t buffer_frames;
  bool started;
} Output;

static HRESULT setup(Output *output) {
  IMMDeviceEnumerator *enumerator = NULL;
  IMMDevice *device = NULL;
  HRESULT result =
      CoCreateInstance(&CLSID_MMDeviceEnumerator, NULL, CLSCTX_ALL,
                       &IID_IMMDeviceEnumerator, (void **)&enumerator);
  if (FAILED(result))
    return result;
  result = IMMDeviceEnumerator_GetDefaultAudioEndpoint(enumerator, eRender,
                                                       eMultimedia, &device);
  if (FAILED(result)) {
    IMMDeviceEnumerator_Release(enumerator);
    return result;
  }
  output->enumerator = enumerator;
  result = hg_watcher_create(enumerator, device, &output->watcher);
  if (FAILED(result)) {
    IMMDevice_Release(device);
    return result;
  }
  result = IMMDevice_Activate(device, &IID_IAudioClient, CLSCTX_ALL, NULL,
                              (void **)&output->client);
  IMMDevice_Release(device);
  if (FAILED(result))
    return result;
  WAVEFORMATEX format = {
      .wFormatTag = WAVE_FORMAT_IEEE_FLOAT,
      .nChannels = (WORD)output->format.channels,
      .nSamplesPerSec = output->format.sample_rate,
      .nAvgBytesPerSec =
          output->format.sample_rate * output->format.channels * sizeof(float),
      .nBlockAlign = (WORD)(output->format.channels * sizeof(float)),
      .wBitsPerSample = sizeof(float) * 8,
      .cbSize = 0,
  };
  result = IAudioClient_Initialize(output->client, AUDCLNT_SHAREMODE_SHARED,
                                   AUDCLNT_STREAMFLAGS_EVENTCALLBACK |
                                       AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM |
                                       AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY |
                                       AUDCLNT_STREAMFLAGS_NOPERSIST,
                                   0, 0, &format, NULL);
  if (FAILED(result))
    return result;
  result = IAudioClient_SetEventHandle(output->client, output->render);
  if (FAILED(result))
    return result;
  result = IAudioClient_GetBufferSize(output->client, &output->buffer_frames);
  if (FAILED(result))
    return result;
  result = IAudioClient_GetService(output->client, &IID_IAudioRenderClient,
                                   (void **)&output->renderer);
  if (FAILED(result))
    return result;
  result = IAudioClient_GetService(output->client, &IID_IAudioClock,
                                   (void **)&output->clock);
  if (FAILED(result))
    return result;
  result = IAudioClock_GetFrequency(output->clock, &output->frequency);
  if (FAILED(result) || !output->frequency)
    return FAILED(result) ? result : E_FAIL;
  return S_OK;
}

static HRESULT played(Output *output, HgPosition *position) {
  if (hg_watcher_changed(output->watcher))
    return AUDCLNT_E_DEVICE_INVALIDATED;
  uint64_t clock = 0;
  HRESULT result = IAudioClock_GetPosition(output->clock, &clock, NULL);
  if (FAILED(result))
    return result;
  uint64_t units = clock >= output->origin ? clock - output->origin : 0;
  uint64_t remainder = units % output->frequency;
  if (remainder > UINT64_MAX / output->format.sample_rate)
    return E_UNEXPECTED;
  position->epoch = output->epoch;
  position->device_frames =
      (units / output->frequency) * output->format.sample_rate +
      remainder * output->format.sample_rate / output->frequency;
  result =
      IAudioClient_GetCurrentPadding(output->client, &position->latency_frames);
  if (FAILED(result))
    return result;
  position->reserved = 0;
  output->callbacks.played(output->callbacks.context, position->epoch,
                           position->device_frames);
  return S_OK;
}

static HRESULT fill(Output *output) {
  HgPosition position;
  HRESULT result = played(output, &position);
  if (FAILED(result))
    return result;
  UINT32 padding = 0;
  result = IAudioClient_GetCurrentPadding(output->client, &padding);
  if (FAILED(result))
    return result;
  if (padding >= output->buffer_frames)
    return S_OK;
  UINT32 frames = output->buffer_frames - padding;
  BYTE *buffer = NULL;
  result = IAudioRenderClient_GetBuffer(output->renderer, frames, &buffer);
  if (FAILED(result))
    return result;
  uint32_t media =
      output->callbacks.read(output->callbacks.context, (float *)buffer, frames,
                             output->format.channels, output->epoch);
  return IAudioRenderClient_ReleaseBuffer(
      output->renderer, frames, media ? 0 : AUDCLNT_BUFFERFLAGS_SILENT);
}

static HRESULT control(Output *output) {
  if (output->operation == POSITION_QUERY)
    return played(output, &output->position);
  if (output->operation == HG_START) {
    if (output->started)
      return S_OK;
    HRESULT result = fill(output);
    if (FAILED(result))
      return result;
    result = IAudioClient_Start(output->client);
    if (SUCCEEDED(result))
      output->started = true;
    return result;
  }
  if (output->operation != HG_PAUSE && output->operation != HG_FLUSH)
    return E_INVALIDARG;
  if (output->started) {
    HRESULT result = IAudioClient_Stop(output->client);
    if (FAILED(result))
      return result;
    output->started = false;
  }
  if (output->operation == HG_PAUSE)
    return played(output, &output->position);
  HRESULT result = IAudioClient_Reset(output->client);
  if (FAILED(result))
    return result;
  result = IAudioClock_GetPosition(output->clock, &output->origin, NULL);
  if (FAILED(result))
    return result;
  output->epoch = output->requested_epoch;
  output->position = (HgPosition){.epoch = output->epoch};
  return S_OK;
}

static void release_interfaces(Output *output) {
  if (output->client && output->started)
    IAudioClient_Stop(output->client);
  if (output->clock)
    IAudioClock_Release(output->clock);
  if (output->renderer)
    IAudioRenderClient_Release(output->renderer);
  if (output->client)
    IAudioClient_Release(output->client);
  if (output->watcher)
    hg_watcher_close(output->enumerator, output->watcher);
  if (output->enumerator)
    IMMDeviceEnumerator_Release(output->enumerator);
}

static DWORD WINAPI worker(void *context) {
  Output *output = context;
  HRESULT initialized = CoInitializeEx(NULL, COINIT_MULTITHREADED);
  output->result = FAILED(initialized) ? initialized : setup(output);
  SetEvent(output->completed);
  if (FAILED(output->result)) {
    release_interfaces(output);
    if (SUCCEEDED(initialized))
      CoUninitialize();
    return 0;
  }
  DWORD task = 0;
  HANDLE priority = AvSetMmThreadCharacteristicsW(L"Audio", &task);
  HANDLE events[] = {output->command, output->render};
  for (;;) {
    DWORD event = WaitForMultipleObjects(2, events, FALSE, INFINITE);
    if (event == WAIT_OBJECT_0) {
      if (output->operation == CLOSE_OUTPUT)
        break;
      output->result = output->failure ? output->failure : control(output);
      SetEvent(output->completed);
    } else if (event == WAIT_OBJECT_0 + 1 && output->started &&
               !output->failure) {
      HRESULT result = fill(output);
      if (FAILED(result)) {
        output->failure = result;
        IAudioClient_Stop(output->client);
        output->started = false;
      }
    } else if (event == WAIT_FAILED) {
      output->failure = HRESULT_FROM_WIN32(GetLastError());
      break;
    }
  }
  if (priority)
    AvRevertMmThreadCharacteristics(priority);
  release_interfaces(output);
  CoUninitialize();
  return 0;
}

static void destroy(Output *output) {
  if (output->thread)
    CloseHandle(output->thread);
  if (output->render)
    CloseHandle(output->render);
  if (output->command)
    CloseHandle(output->command);
  if (output->completed)
    CloseHandle(output->completed);
  DeleteCriticalSection(&output->lock);
  free(output);
}

int32_t hg_pcm_output_open(HgFormat format, HgCallbacks callbacks,
                           void **result) {
  if (!result || !format.sample_rate || !format.channels || !callbacks.read ||
      !callbacks.played)
    return HG_INVALID;
  if (format.channels > 2 ||
      (uint64_t)format.sample_rate * format.channels * sizeof(float) >
          UINT32_MAX)
    return HG_FORMAT;
  *result = NULL;
  Output *output = calloc(1, sizeof(Output));
  if (!output)
    return HG_NO_MEMORY;
  output->format = format;
  output->callbacks = callbacks;
  InitializeCriticalSection(&output->lock);
  output->command = CreateEventW(NULL, FALSE, FALSE, NULL);
  output->completed = CreateEventW(NULL, FALSE, FALSE, NULL);
  output->render = CreateEventW(NULL, FALSE, FALSE, NULL);
  if (!output->command || !output->completed || !output->render) {
    int32_t error = HRESULT_FROM_WIN32(GetLastError());
    destroy(output);
    return error;
  }
  output->thread = CreateThread(NULL, 0, worker, output, 0, NULL);
  if (!output->thread) {
    int32_t error = HRESULT_FROM_WIN32(GetLastError());
    destroy(output);
    return error;
  }
  HANDLE handles[] = {output->completed, output->thread};
  DWORD event = WaitForMultipleObjects(2, handles, FALSE, INFINITE);
  if (event != WAIT_OBJECT_0 || FAILED(output->result)) {
    int32_t error = FAILED(output->result) ? output->result : HG_DEVICE;
    WaitForSingleObject(output->thread, INFINITE);
    destroy(output);
    return error;
  }
  *result = output;
  return S_OK;
}

static int32_t request(Output *output, int32_t operation, uint64_t epoch) {
  EnterCriticalSection(&output->lock);
  output->operation = operation;
  output->requested_epoch = epoch;
  if (!SetEvent(output->command)) {
    int32_t error = HRESULT_FROM_WIN32(GetLastError());
    LeaveCriticalSection(&output->lock);
    return error;
  }
  HANDLE handles[] = {output->completed, output->thread};
  DWORD event = WaitForMultipleObjects(2, handles, FALSE, INFINITE);
  int32_t result = event == WAIT_OBJECT_0 ? output->result : HG_DEVICE;
  LeaveCriticalSection(&output->lock);
  return result;
}

int32_t hg_pcm_output_control(void *handle, int32_t operation, uint64_t epoch) {
  if (!handle)
    return HG_INVALID;
  return request(handle, operation, epoch);
}

int32_t hg_pcm_output_position(void *handle, HgPosition *position) {
  Output *output = handle;
  if (!output || !position)
    return HG_INVALID;
  int32_t result = request(output, POSITION_QUERY, 0);
  if (!result)
    *position = output->position;
  return result;
}

void hg_pcm_output_close(void *handle) {
  Output *output = handle;
  if (!output)
    return;
  EnterCriticalSection(&output->lock);
  output->operation = CLOSE_OUTPUT;
  SetEvent(output->command);
  WaitForSingleObject(output->thread, INFINITE);
  LeaveCriticalSection(&output->lock);
  destroy(output);
}
