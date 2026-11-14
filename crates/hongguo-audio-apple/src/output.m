#include "output.h"
#include <AudioToolbox/AudioToolbox.h>
#include <TargetConditionals.h>
#include <mach/mach_time.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#if TARGET_OS_OSX
#include <CoreAudio/AudioHardware.h>
#else
#import <AVFAudio/AVAudioSession.h>
#endif

typedef struct {
  AudioUnit unit;
  HgFormat format;
  HgCallbacks callbacks;
  atomic_uint_fast64_t epoch, submitted, played, anchor_frame, anchor_ns,
      sequence, silence_ns;
  atomic_bool running, reading;
  atomic_int error;
  uint32_t latency, device;
  mach_timebase_info_data_t timebase;
  bool initialized;
} Output;

static uint64_t nanos(Output *output, uint64_t ticks) {
  return (uint64_t)((__uint128_t)ticks * output->timebase.numer /
                    output->timebase.denom);
}

static uint64_t host_ns(Output *output) {
  return nanos(output, mach_absolute_time());
}

static uint64_t consumed(Output *output) {
  uint64_t previous = atomic_load(&output->played);
  if (!atomic_load(&output->running))
    return previous;
  uint64_t sequence, frame, time;
  do {
    sequence = atomic_load(&output->sequence);
    if (sequence & 1)
      continue;
    frame = atomic_load(&output->anchor_frame);
    time = atomic_load(&output->anchor_ns);
  } while ((sequence & 1) || sequence != atomic_load(&output->sequence));
  if (!time)
    return previous;
  uint64_t now = host_ns(output);
  uint64_t distance =
      (uint64_t)(((__uint128_t)(now > time ? now - time : time - now) *
                  output->format.sample_rate) /
                 1000000000);
  frame = now >= time        ? frame + distance
          : frame > distance ? frame - distance
                             : 0;
  uint64_t submitted = atomic_load(&output->submitted);
  if (frame > submitted)
    frame = submitted;
  while (frame > previous &&
         !atomic_compare_exchange_weak(&output->played, &previous, frame)) {
  }
  return frame > previous ? frame : previous;
}

static OSStatus render(void *context, AudioUnitRenderActionFlags *flags,
                       const AudioTimeStamp *timestamp, UInt32 bus,
                       UInt32 frames, AudioBufferList *buffers) {
  (void)bus;
  Output *output = context;
  if (!buffers || buffers->mNumberBuffers != 1) {
    atomic_store(&output->error, HG_FORMAT);
    return kAudio_ParamError;
  }
  AudioBuffer *buffer = &buffers->mBuffers[0];
  size_t bytes = (size_t)frames * output->format.channels * sizeof(float);
  if (!buffer->mData || buffer->mDataByteSize < bytes) {
    atomic_store(&output->error, HG_FORMAT);
    return kAudio_ParamError;
  }
  memset(buffer->mData, 0, bytes);
  if (!(timestamp->mFlags & kAudioTimeStampHostTimeValid)) {
    atomic_store(&output->error, HG_DEVICE);
    *flags |= kAudioUnitRenderAction_OutputIsSilence;
    return noErr;
  }
  uint64_t epoch = atomic_load(&output->epoch);
  output->callbacks.played(output->callbacks.context, epoch, consumed(output));
  if (!atomic_load(&output->reading)) {
    uint64_t empty = 0;
    if (timestamp->mFlags & kAudioTimeStampHostTimeValid)
      atomic_compare_exchange_strong(&output->silence_ns, &empty,
                                     nanos(output, timestamp->mHostTime));
    *flags |= kAudioUnitRenderAction_OutputIsSilence;
    return noErr;
  }
  uint64_t first = atomic_load(&output->submitted);
  if (timestamp->mFlags & kAudioTimeStampHostTimeValid) {
    atomic_fetch_add(&output->sequence, 1);
    atomic_store(&output->anchor_frame, first);
    atomic_store(&output->anchor_ns, nanos(output, timestamp->mHostTime));
    atomic_fetch_add(&output->sequence, 1);
  }
  uint32_t media =
      output->callbacks.read(output->callbacks.context, buffer->mData, frames,
                             output->format.channels, epoch);
  atomic_fetch_add(&output->submitted, frames);
  if (!media)
    *flags |= kAudioUnitRenderAction_OutputIsSilence;
  return noErr;
}

static int32_t configure(Output *output) {
  AudioComponentDescription description = {
      .componentType = kAudioUnitType_Output,
#if TARGET_OS_OSX
      .componentSubType = kAudioUnitSubType_HALOutput,
#else
      .componentSubType = kAudioUnitSubType_RemoteIO,
#endif
      .componentManufacturer = kAudioUnitManufacturer_Apple,
  };
  AudioComponent component = AudioComponentFindNext(NULL, &description);
  if (!component)
    return HG_DEVICE;
  OSStatus status = AudioComponentInstanceNew(component, &output->unit);
  if (status)
    return status;
#if TARGET_OS_OSX
  AudioObjectPropertyAddress address = {
      kAudioHardwarePropertyDefaultOutputDevice,
      kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
  AudioDeviceID device = kAudioObjectUnknown;
  UInt32 size = sizeof(device);
  status = AudioObjectGetPropertyData(kAudioObjectSystemObject, &address, 0,
                                      NULL, &size, &device);
  if (status)
    return status;
  status =
      AudioUnitSetProperty(output->unit, kAudioOutputUnitProperty_CurrentDevice,
                           kAudioUnitScope_Global, 0, &device, sizeof(device));
  if (status)
    return status;
#else
  NSError *error = nil;
  if (![[AVAudioSession sharedInstance]
          setCategory:AVAudioSessionCategoryPlayback
                 mode:AVAudioSessionModeMoviePlayback
              options:0
                error:&error])
    return HG_DEVICE;
#endif
#if TARGET_OS_OSX
  output->device = device;
#endif
  AudioStreamBasicDescription format = {
      .mSampleRate = output->format.sample_rate,
      .mFormatID = kAudioFormatLinearPCM,
      .mFormatFlags = kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
      .mBytesPerPacket = output->format.channels * sizeof(float),
      .mFramesPerPacket = 1,
      .mBytesPerFrame = output->format.channels * sizeof(float),
      .mChannelsPerFrame = output->format.channels,
      .mBitsPerChannel = 8 * sizeof(float),
  };
  status =
      AudioUnitSetProperty(output->unit, kAudioUnitProperty_StreamFormat,
                           kAudioUnitScope_Input, 0, &format, sizeof(format));
  if (status)
    return status;
  AURenderCallbackStruct callback = {render, output};
  status = AudioUnitSetProperty(
      output->unit, kAudioUnitProperty_SetRenderCallback, kAudioUnitScope_Input,
      0, &callback, sizeof(callback));
  if (status)
    return status;
  status = AudioUnitInitialize(output->unit);
  if (status)
    return status;
  output->initialized = true;
  Float64 latency = 0;
  UInt32 latency_size = sizeof(latency);
  status =
      AudioUnitGetProperty(output->unit, kAudioUnitProperty_Latency,
                           kAudioUnitScope_Global, 0, &latency, &latency_size);
  if (status)
    return status;
  output->latency = (uint32_t)(latency * output->format.sample_rate);
  return noErr;
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
  if (mach_timebase_info(&output->timebase) != KERN_SUCCESS ||
      !output->timebase.denom) {
    free(output);
    return HG_DEVICE;
  }
  int32_t status = configure(output);
  if (status) {
    hg_pcm_output_close(output);
    return status;
  }
  *result = output;
  return noErr;
}

static int32_t pause_output(Output *output) {
  if (!atomic_load(&output->running))
    return noErr;
  atomic_store(&output->silence_ns, 0);
  atomic_store(&output->reading, false);
  uint64_t deadline = host_ns(output) + 1000000000;
  while (host_ns(output) < deadline) {
    uint64_t silence = atomic_load(&output->silence_ns);
    if (silence && host_ns(output) >= silence)
      break;
    struct timespec delay = {0, 1000000};
    nanosleep(&delay, NULL);
  }
  consumed(output);
  OSStatus status = AudioOutputUnitStop(output->unit);
  atomic_store(&output->running, false);
  uint64_t silence = atomic_load(&output->silence_ns);
  return status                                  ? status
         : silence && host_ns(output) >= silence ? noErr
                                                 : HG_DEVICE;
}

int32_t hg_pcm_output_control(void *handle, int32_t operation, uint64_t epoch) {
  Output *output = handle;
  if (!output)
    return HG_INVALID;
  if (atomic_load(&output->error))
    return atomic_load(&output->error);
  if (operation == HG_PAUSE)
    return pause_output(output);
  if (operation == HG_START) {
    if (atomic_load(&output->running))
      return noErr;
#if !TARGET_OS_OSX
    NSError *error = nil;
    if (![[AVAudioSession sharedInstance] setActive:YES error:&error])
      return HG_DEVICE;
#endif
    atomic_store(&output->running, true);
    atomic_store(&output->reading, true);
    OSStatus status = AudioOutputUnitStart(output->unit);
    if (status) {
      atomic_store(&output->running, false);
      atomic_store(&output->reading, false);
    }
    return status;
  }
  if (operation != HG_FLUSH)
    return HG_INVALID;
  OSStatus status = AudioOutputUnitStop(output->unit);
  if (status)
    return status;
  atomic_store(&output->running, false);
  atomic_store(&output->reading, false);
  status = AudioUnitReset(output->unit, kAudioUnitScope_Global, 0);
  if (status)
    return status;
  atomic_store(&output->epoch, epoch);
  atomic_store(&output->submitted, 0);
  atomic_store(&output->played, 0);
  atomic_store(&output->anchor_ns, 0);
  atomic_store(&output->anchor_frame, 0);
  atomic_store(&output->silence_ns, 0);
  return noErr;
}

int32_t hg_pcm_output_position(void *handle, HgPosition *position) {
  Output *output = handle;
  if (!output || !position)
    return HG_INVALID;
  if (atomic_load(&output->error))
    return atomic_load(&output->error);
#if TARGET_OS_OSX
  AudioObjectPropertyAddress address = {
      kAudioHardwarePropertyDefaultOutputDevice,
      kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
  AudioDeviceID device = kAudioObjectUnknown;
  UInt32 size = sizeof(device);
  OSStatus status = AudioObjectGetPropertyData(
      kAudioObjectSystemObject, &address, 0, NULL, &size, &device);
  if (status)
    return status;
  if (device != output->device)
    return HG_DEVICE;
#endif
  if (atomic_load(&output->running)) {
    UInt32 running = 0, size = sizeof(running);
    OSStatus status =
        AudioUnitGetProperty(output->unit, kAudioOutputUnitProperty_IsRunning,
                             kAudioUnitScope_Global, 0, &running, &size);
    if (status)
      return status;
    if (!running)
      return HG_DEVICE;
  }
  position->epoch = atomic_load(&output->epoch);
  position->device_frames = consumed(output);
  position->latency_frames = output->latency;
  position->reserved = 0;
  output->callbacks.played(output->callbacks.context, position->epoch,
                           position->device_frames);
  return noErr;
}

void hg_pcm_output_close(void *handle) {
  Output *output = handle;
  if (!output)
    return;
  if (output->unit) {
    AudioOutputUnitStop(output->unit);
    if (output->initialized)
      AudioUnitUninitialize(output->unit);
    AudioComponentInstanceDispose(output->unit);
  }
#if !TARGET_OS_OSX
  [[AVAudioSession sharedInstance]
        setActive:NO
      withOptions:AVAudioSessionSetActiveOptionNotifyOthersOnDeactivation
            error:NULL];
#endif
  free(output);
}
