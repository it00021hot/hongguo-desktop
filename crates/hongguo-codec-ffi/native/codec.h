#ifndef HONGGUO_CODEC_H
#define HONGGUO_CODEC_H
#include <stdint.h>
#include <stddef.h>
#include <libavcodec/avcodec.h>
#include <libavutil/frame.h>

enum { HG_SOFTWARE, HG_VIDEOTOOLBOX, HG_D3D11, HG_MEDIACODEC, HG_D3D12 };
enum { HG_HEVC, HG_H264 };
typedef struct HgVideo HgVideo;
typedef struct HgAudio HgAudio;
typedef struct {
    int width, height, bit_depth, chroma_w, chroma_h, planes;
    int crop_left, crop_top, crop_right, crop_bottom;
    int primaries, transfer, matrix, full_range, chroma_location;
    int sar_num, sar_den, backend;
    int64_t pts, duration;
    uintptr_t handle, subresource;
    const uint8_t *data[4];
    int stride[4], plane_width[4], plane_height[4], bytes_per_sample;
    int sample_step[4], sample_shift[4], big_endian;
    int has_mastering, has_content_light;
    uint16_t mastering_primaries[6], white_point[2];
    uint32_t max_luminance, min_luminance;
    uint16_t max_cll, max_fall;
} HgFrameInfo;
typedef struct {
    const float *samples;
    int frames, channels, sample_rate;
    int64_t pts;
    double media_start_secs, media_duration_secs;
} HgPcmInfo;
typedef struct { uintptr_t texture; uint32_t subresource; uintptr_t fence; uint64_t fence_value; } HgD3d12Frame;

int hg_video_open(HgVideo **out, int codec, int backend, uintptr_t device,
                  uintptr_t surface, const uint8_t *extra, int extra_size,
                  int time_num, int time_den, int threads);
void hg_video_free(HgVideo **video);
int hg_video_send(HgVideo *video, const uint8_t *data, int size,
                  int64_t pts, int64_t dts, int64_t duration);
int hg_video_receive(HgVideo *video, AVFrame **frame);
void hg_video_flush(HgVideo *video);
int64_t hg_mediacodec_timestamp_key(int64_t pts, int time_num, int time_den);
int hg_frame_info(const AVFrame *frame, HgFrameInfo *info);
void hg_frame_free(AVFrame **frame);
int hg_frame_download(const AVFrame *frame, AVFrame **out);
int hg_frame_render(AVFrame *frame, int64_t time_ns);
int hg_frame_d3d12(const AVFrame *frame, HgD3d12Frame *info);
int hg_error_string(int code, char *buffer, size_t size);
int hg_audio_open(HgAudio **out, const uint8_t *extra, int extra_size,
                  int input_rate, int output_rate, int channels, double rate);
void hg_audio_free(HgAudio **audio);
int hg_audio_send(HgAudio *audio, const uint8_t *data, int size, int64_t pts);
int hg_audio_receive(HgAudio *audio, HgPcmInfo *info);
int hg_audio_reset(HgAudio *audio, double rate);
#endif
