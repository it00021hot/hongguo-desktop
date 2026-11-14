#define COBJMACROS
#include "codec.h"
#include <errno.h>
#include <string.h>
#include <libavutil/error.h>
#include <libavutil/hwcontext.h>
#include <libavutil/mastering_display_metadata.h>
#include <libavutil/mathematics.h>
#include <libavutil/mem.h>
#include <libavutil/pixdesc.h>
#if defined(_WIN32)
#include <libavutil/hwcontext_d3d11va.h>
#include <libavutil/hwcontext_d3d12va.h>
#endif
#if defined(__ANDROID__)
#include <libavcodec/jni.h>
#include <libavcodec/mediacodec.h>
#endif

enum { HG_PACKET_FACT_CAPACITY = 128 };
typedef struct { int64_t pts, key, duration; enum AVPixelFormat format; int occupied; } HgPacketFact;

int64_t hg_mediacodec_timestamp_key(int64_t pts, int time_num, int time_den) {
    if (pts == AV_NOPTS_VALUE) return pts;
    AVRational time_base = { time_num, time_den };
    // MediaCodec accepts microseconds. Match its round trip rather than exact
    // source ticks, then restore the original timestamp on the decoded frame.
    return av_rescale_q(av_rescale_q(pts, time_base, AV_TIME_BASE_Q), AV_TIME_BASE_Q, time_base);
}

struct HgVideo {
    AVCodecContext *codec;
    enum AVPixelFormat desired;
    int backend;
    AVCodecParserContext *parser;
    AVCodecContext *parser_context;
    HgPacketFact facts[HG_PACKET_FACT_CAPACITY];
};

static enum AVPixelFormat select_format(AVCodecContext *context,
                                        const enum AVPixelFormat *formats) {
    HgVideo *video = context->opaque;
    for (; *formats != AV_PIX_FMT_NONE; ++formats) {
        if (video->backend == HG_SOFTWARE) {
            const AVPixFmtDescriptor *description = av_pix_fmt_desc_get(*formats);
            if (description && !(description->flags & AV_PIX_FMT_FLAG_HWACCEL)) return *formats;
        } else if (*formats == video->desired) {
            return *formats;
        }
    }
    return AV_PIX_FMT_NONE;
}

static int setup_hardware(HgVideo *video, uintptr_t device, uintptr_t surface) {
    (void)device;
    (void)surface;
    AVBufferRef *reference = NULL;
    int result = AVERROR(ENOSYS);
    switch (video->backend) {
    case HG_VIDEOTOOLBOX:
#if defined(__APPLE__)
        video->desired = AV_PIX_FMT_VIDEOTOOLBOX;
        result = av_hwdevice_ctx_create(&reference, AV_HWDEVICE_TYPE_VIDEOTOOLBOX, NULL, NULL, 0);
#endif
        break;
    case HG_D3D11:
#if defined(_WIN32)
        if (!device) return AVERROR(EINVAL);
        video->desired = AV_PIX_FMT_D3D11;
        reference = av_hwdevice_ctx_alloc(AV_HWDEVICE_TYPE_D3D11VA);
        if (!reference) return AVERROR(ENOMEM);
        AVHWDeviceContext *context = (AVHWDeviceContext *)reference->data;
        AVD3D11VADeviceContext *d3d = context->hwctx;
        d3d->device = (ID3D11Device *)device;
        ID3D11Device_AddRef(d3d->device);
        // Decoder arrays are copied on-GPU to independent shader resources.
        d3d->BindFlags = D3D11_BIND_DECODER;
        result = av_hwdevice_ctx_init(reference);
#endif
        break;
    case HG_MEDIACODEC:
#if defined(__ANDROID__)
        if (!device || !surface) return AVERROR(EINVAL);
        result = av_jni_set_java_vm((void *)device, NULL);
        if (result < 0) return result;
        AVMediaCodecContext *media = av_mediacodec_alloc_context();
        if (!media) return AVERROR(ENOMEM);
        video->desired = AV_PIX_FMT_MEDIACODEC;
        result = av_mediacodec_default_init(video->codec, media, (void *)surface);
        // FFmpeg attaches this allocation to hwaccel_context only on success.
        if (result < 0) av_free(media);
#endif
        break;
    case HG_D3D12:
#if defined(_WIN32)
        if (!device) return AVERROR(EINVAL);
        video->desired = AV_PIX_FMT_D3D12;
        reference = av_hwdevice_ctx_alloc(AV_HWDEVICE_TYPE_D3D12VA);
        if (!reference) return AVERROR(ENOMEM);
        AVHWDeviceContext *d12_context = (AVHWDeviceContext *)reference->data;
        AVD3D12VADeviceContext *d12 = d12_context->hwctx;
        d12->device = (ID3D12Device *)device;
        d12->resource_flags = D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS;
        ID3D12Device_AddRef(d12->device);
        result = av_hwdevice_ctx_init(reference);
#endif
        break;
    default:
        return AVERROR(EINVAL);
    }
    if (result < 0) {
        av_buffer_unref(&reference);
        return result;
    }
    video->codec->hw_device_ctx = reference;
    return 0;
}

int hg_video_open(HgVideo **out, int codec, int backend, uintptr_t device,
                  uintptr_t surface, const uint8_t *extra, int extra_size,
                  int time_num, int time_den, int threads) {
    if (!out || extra_size < 0 || time_num <= 0 || time_den <= 0 || threads < 0)
        return AVERROR(EINVAL);
    *out = NULL;
    const char *name = codec == HG_HEVC ? "hevc" : codec == HG_H264 ? "h264" : NULL;
    if (!name) return AVERROR(EINVAL);
#if defined(__ANDROID__)
    if (backend == HG_MEDIACODEC) name = codec == HG_HEVC ? "hevc_mediacodec" : "h264_mediacodec";
#endif
    const AVCodec *implementation = avcodec_find_decoder_by_name(name);
    if (!implementation) return AVERROR_DECODER_NOT_FOUND;
    HgVideo *video = av_mallocz(sizeof(*video));
    if (!video) return AVERROR(ENOMEM);
    video->backend = backend;
    video->codec = avcodec_alloc_context3(implementation);
    if (!video->codec) { av_free(video); return AVERROR(ENOMEM); }
    video->codec->opaque = video;
    video->codec->get_format = select_format;
    video->codec->pkt_timebase = (AVRational){ time_num, time_den };
    video->codec->thread_count = backend == HG_SOFTWARE ? threads : 1;
    video->codec->thread_type = FF_THREAD_FRAME | FF_THREAD_SLICE;
    video->codec->extra_hw_frames = 6;
    if (extra_size) {
        video->codec->extradata = av_mallocz((size_t)extra_size + AV_INPUT_BUFFER_PADDING_SIZE);
        if (!video->codec->extradata) { hg_video_free(&video); return AVERROR(ENOMEM); }
        memcpy(video->codec->extradata, extra, extra_size);
        video->codec->extradata_size = extra_size;
    }
    if (backend == HG_MEDIACODEC) {
        video->parser = av_parser_init(implementation->id);
        video->parser_context = avcodec_alloc_context3(NULL);
        if (!video->parser || !video->parser_context) { hg_video_free(&video); return AVERROR(ENOMEM); }
        video->parser->flags = PARSER_FLAG_COMPLETE_FRAMES;
        video->parser->format = AV_PIX_FMT_NONE;
        video->parser_context->codec_id = implementation->id;
        video->parser_context->pkt_timebase = video->codec->pkt_timebase;
        if (extra_size) {
            video->parser_context->extradata = av_memdup(video->codec->extradata, (size_t)extra_size + AV_INPUT_BUFFER_PADDING_SIZE);
            if (!video->parser_context->extradata) { hg_video_free(&video); return AVERROR(ENOMEM); }
            video->parser_context->extradata_size = extra_size;
        }
    }
    int result = backend == HG_SOFTWARE ? 0 : setup_hardware(video, device, surface);
    if (result >= 0) result = avcodec_open2(video->codec, implementation, NULL);
    if (result < 0) { hg_video_free(&video); return result; }
    *out = video;
    return 0;
}

void hg_video_free(HgVideo **video) {
    if (!video || !*video) return;
#if defined(__ANDROID__)
    if ((*video)->backend == HG_MEDIACODEC) av_mediacodec_default_free((*video)->codec);
#endif
    avcodec_free_context(&(*video)->codec);
    avcodec_free_context(&(*video)->parser_context);
    if ((*video)->parser) av_parser_close((*video)->parser);
    av_freep(video);
}

int hg_video_send(HgVideo *video, const uint8_t *data, int size,
                  int64_t pts, int64_t dts, int64_t duration) {
    if (size < 0) return AVERROR(EINVAL);
    AVPacket *packet = NULL;
    int fact_index = -1;
    if (data) {
        packet = av_packet_alloc();
        if (!packet) return AVERROR(ENOMEM);
        int result = av_new_packet(packet, size);
        if (result < 0) { av_packet_free(&packet); return result; }
        memcpy(packet->data, data, size);
        packet->pts = pts;
        packet->dts = dts;
        packet->duration = duration;
        if (video->parser) {
            uint8_t *parsed = NULL;
            int parsed_size = 0;
            result = av_parser_parse2(video->parser, video->parser_context, &parsed, &parsed_size,
                                      packet->data, size, pts, dts, -1);
            if (result < 0 || pts == AV_NOPTS_VALUE || video->parser->format == AV_PIX_FMT_NONE) {
                av_packet_free(&packet); return result < 0 ? result : AVERROR(ENOSYS);
            }
            for (int index = 0; index < HG_PACKET_FACT_CAPACITY; ++index) {
                if (!video->facts[index].occupied) { fact_index = index; break; }
            }
            if (fact_index < 0) { av_packet_free(&packet); return AVERROR(ENOBUFS); }
        }
    }
    int result = avcodec_send_packet(video->codec, packet);
    av_packet_free(&packet);
    if (result >= 0 && fact_index >= 0) {
        AVRational time_base = video->codec->pkt_timebase;
        video->facts[fact_index] = (HgPacketFact){ pts,
            hg_mediacodec_timestamp_key(pts, time_base.num, time_base.den), duration, video->parser->format, 1 };
    }
    return result == AVERROR(EAGAIN) ? 0 : result < 0 ? result : 1;
}

int hg_video_receive(HgVideo *video, AVFrame **out) {
    *out = av_frame_alloc();
    if (!*out) return AVERROR(ENOMEM);
    int result = avcodec_receive_frame(video->codec, *out);
    if (result >= 0 && video->parser) {
        int found = 0;
        for (int index = 0; index < HG_PACKET_FACT_CAPACITY; ++index) {
            HgPacketFact *fact = &video->facts[index];
            if (!fact->occupied || fact->key != (*out)->pts) continue;
            if ((*out)->opaque_ref) { result = AVERROR_BUG; break; }
            (*out)->opaque_ref = av_buffer_alloc(sizeof(*fact));
            if (!(*out)->opaque_ref) { result = AVERROR(ENOMEM); break; }
            memcpy((*out)->opaque_ref->data, fact, sizeof(*fact));
            (*out)->pts = fact->pts;
            (*out)->best_effort_timestamp = fact->pts;
            if (fact->duration > 0) (*out)->duration = fact->duration;
            fact->occupied = 0;
            found = 1;
            break;
        }
        if (result >= 0 && !found) result = AVERROR(ENOSYS);
    }
    if (result < 0) av_frame_free(out);
    return result == AVERROR(EAGAIN) || result == AVERROR_EOF ? 0 : result < 0 ? result : 1;
}

void hg_video_flush(HgVideo *video) { avcodec_flush_buffers(video->codec); memset(video->facts, 0, sizeof(video->facts)); }
void hg_frame_free(AVFrame **frame) { av_frame_free(frame); }
int hg_error_string(int code, char *buffer, size_t size) { return av_strerror(code, buffer, size); }

int hg_frame_download(const AVFrame *frame, AVFrame **out) {
    *out = av_frame_alloc();
    if (!*out) return AVERROR(ENOMEM);
    int result = av_hwframe_transfer_data(*out, frame, 0);
    if (result >= 0) result = av_frame_copy_props(*out, frame);
    if (result < 0) av_frame_free(out);
    return result;
}

static uint32_t rational_scale(AVRational value, double scale) {
    double result = av_q2d(value) * scale;
    return result < 0 ? 0 : result > UINT32_MAX ? UINT32_MAX : (uint32_t)(result + 0.5);
}

int hg_frame_info(const AVFrame *frame, HgFrameInfo *info) {
    memset(info, 0, sizeof(*info));
    info->width = frame->width;
    info->height = frame->height;
    info->pts = frame->best_effort_timestamp;
    info->duration = frame->duration;
    info->crop_left = (int)frame->crop_left;
    info->crop_top = (int)frame->crop_top;
    info->crop_right = (int)frame->crop_right;
    info->crop_bottom = (int)frame->crop_bottom;
    info->primaries = frame->color_primaries;
    info->transfer = frame->color_trc;
    info->matrix = frame->colorspace;
    info->full_range = frame->color_range == AVCOL_RANGE_UNSPECIFIED ? -1 : frame->color_range == AVCOL_RANGE_JPEG;
    info->chroma_location = frame->chroma_location;
    info->sar_num = frame->sample_aspect_ratio.num;
    info->sar_den = frame->sample_aspect_ratio.den;
    enum AVPixelFormat format = frame->format;
    switch (format) {
    case AV_PIX_FMT_VIDEOTOOLBOX:
        info->backend = HG_VIDEOTOOLBOX;
        info->handle = (uintptr_t)frame->data[3];
        break;
    case AV_PIX_FMT_D3D11:
        info->backend = HG_D3D11;
        info->handle = (uintptr_t)frame->data[0];
        info->subresource = (uintptr_t)frame->data[1];
        break;
    case AV_PIX_FMT_MEDIACODEC:
        info->backend = HG_MEDIACODEC;
        info->handle = (uintptr_t)frame->data[3];
        break;
    case AV_PIX_FMT_D3D12:
        info->backend = HG_D3D12;
        info->handle = (uintptr_t)frame->data[0];
        break;
    default: break;
    }
    if (info->backend && frame->hw_frames_ctx) {
        AVHWFramesContext *context = (AVHWFramesContext *)frame->hw_frames_ctx->data;
        format = context->sw_format;
    } else if (info->backend == HG_MEDIACODEC && frame->opaque_ref && frame->opaque_ref->size == sizeof(HgPacketFact)) {
        const HgPacketFact *fact = (const void *)frame->opaque_ref->data;
        format = fact->format;
    }
    const AVPixFmtDescriptor *description = av_pix_fmt_desc_get(format);
    if (!description || (description->flags & AV_PIX_FMT_FLAG_RGB)) return AVERROR(ENOSYS);
    info->bit_depth = description->comp[0].depth;
    info->chroma_w = description->log2_chroma_w;
    info->chroma_h = description->log2_chroma_h;
    info->planes = description->nb_components;
    info->bytes_per_sample = info->bit_depth > 8 ? 2 : 1;
    info->big_endian = !!(description->flags & AV_PIX_FMT_FLAG_BE);
    if (!info->backend) {
        for (int component = 0; component < info->planes && component < 4; ++component) {
            AVComponentDescriptor descriptor = description->comp[component];
            int chroma = component == 1 || component == 2;
            int shift_w = chroma ? info->chroma_w : 0;
            int shift_h = chroma ? info->chroma_h : 0;
            info->data[component] = frame->data[descriptor.plane] + descriptor.offset;
            info->stride[component] = frame->linesize[descriptor.plane];
            info->sample_step[component] = descriptor.step;
            info->sample_shift[component] = descriptor.shift;
            info->plane_width[component] = (frame->width + (1 << shift_w) - 1) >> shift_w;
            info->plane_height[component] = (frame->height + (1 << shift_h) - 1) >> shift_h;
        }
    }
    AVFrameSideData *side = av_frame_get_side_data(frame, AV_FRAME_DATA_MASTERING_DISPLAY_METADATA);
    if (side && side->size >= sizeof(AVMasteringDisplayMetadata)) {
        const AVMasteringDisplayMetadata *master = (void *)side->data;
        if (master->has_primaries && master->has_luminance) {
            info->has_mastering = 1;
            for (int p = 0; p < 3; ++p) for (int c = 0; c < 2; ++c)
                info->mastering_primaries[p * 2 + c] = (uint16_t)rational_scale(master->display_primaries[p][c], 50000);
            for (int c = 0; c < 2; ++c) info->white_point[c] = (uint16_t)rational_scale(master->white_point[c], 50000);
            info->max_luminance = rational_scale(master->max_luminance, 10000);
            info->min_luminance = rational_scale(master->min_luminance, 10000);
        }
    }
    side = av_frame_get_side_data(frame, AV_FRAME_DATA_CONTENT_LIGHT_LEVEL);
    if (side && side->size >= sizeof(AVContentLightMetadata)) {
        const AVContentLightMetadata *light = (void *)side->data;
        info->has_content_light = 1;
        info->max_cll = light->MaxCLL;
        info->max_fall = light->MaxFALL;
    }
    return 0;
}

int hg_frame_render(AVFrame *frame, int64_t time_ns) {
    (void)frame;
    (void)time_ns;
#if defined(__ANDROID__)
    if (frame->format == AV_PIX_FMT_MEDIACODEC)
        return av_mediacodec_render_buffer_at_time((void *)frame->data[3], time_ns);
#endif
    return AVERROR(ENOSYS);
}

int hg_frame_d3d12(const AVFrame *frame, HgD3d12Frame *info) {
    (void)frame; (void)info;
#if defined(_WIN32)
    if (frame->format != AV_PIX_FMT_D3D12 || !frame->data[0]) return AVERROR(EINVAL);
    const AVD3D12VAFrame *surface = (const void *)frame->data[0];
    info->texture = (uintptr_t)surface->texture;
    info->subresource = (uint32_t)surface->subresource_index;
    info->fence = (uintptr_t)surface->sync_ctx.fence;
    info->fence_value = surface->sync_ctx.fence_value;
    return 0;
#else
    return AVERROR(ENOSYS);
#endif
}
