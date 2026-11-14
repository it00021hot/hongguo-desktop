#include "codec.h"
#include <errno.h>
#include <math.h>
#include <stdio.h>
#include <string.h>
#include <libavfilter/avfilter.h>
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
#include <libavutil/channel_layout.h>
#include <libavutil/mem.h>
#include <libswresample/swresample.h>

enum { HG_ATEMPO_MAX_STAGES = 32, HG_ATEMPO_FILTER_BYTES = 2048 };

static int tempo_stages(double rate) {
    if (!isfinite(rate) || rate <= 0.0) return -1;
    double stages = ceil(fabs(log2(rate)));
    return stages > HG_ATEMPO_MAX_STAGES ? -1 : stages < 1 ? 1 : (int)stages;
}

struct HgAudio {
    AVCodecContext *codec;
    SwrContext *resampler;
    AVFilterGraph *graph;
    AVFilterContext *source, *sink;
    AVFrame *current;
    AVFrame *pending;
    AVChannelLayout output_layout, input_layout;
    int input_rate, output_rate, input_format, channels;
    int decoder_eof, resampler_eof, graph_eof;
    int segment_draining, has_anchor;
    double anchor_secs, expected_secs;
    int64_t output_frames;
    double rate;
};

static int create_filter(HgAudio *audio) {
    if (audio->rate == 1.0) return 0;
    char layout[128], arguments[256], filters[HG_ATEMPO_FILTER_BYTES];
    int result = av_channel_layout_describe(&audio->output_layout, layout, sizeof(layout));
    if (result < 0) return result;
    snprintf(arguments, sizeof(arguments), "time_base=1/%d:sample_rate=%d:sample_fmt=flt:channel_layout=%s",
             audio->output_rate, audio->output_rate, layout);
    int stages = tempo_stages(audio->rate);
    if (stages < 0) return AVERROR(EINVAL);
    double stage = pow(audio->rate, 1.0 / stages);
    if (stage < 0.5 || stage > 2.0) {
        if (++stages > HG_ATEMPO_MAX_STAGES) return AVERROR(EINVAL);
        stage = pow(audio->rate, 1.0 / stages);
    }
    size_t used = 0;
    for (int index = 0; index < stages; ++index) {
        int written = snprintf(filters + used, sizeof(filters) - used, "%satempo=%.17g", index ? "," : "", stage);
        if (written < 0 || (size_t)written >= sizeof(filters) - used) return AVERROR(EINVAL);
        used += written;
    }
    audio->graph = avfilter_graph_alloc();
    if (!audio->graph) return AVERROR(ENOMEM);
    audio->graph->nb_threads = 1;
    result = avfilter_graph_create_filter(&audio->source, avfilter_get_by_name("abuffer"),
                                          "input", arguments, NULL, audio->graph);
    if (result < 0) return result;
    result = avfilter_graph_create_filter(&audio->sink, avfilter_get_by_name("abuffersink"),
                                          "output", NULL, NULL, audio->graph);
    if (result < 0) return result;
    AVFilterInOut *inputs = avfilter_inout_alloc(), *outputs = avfilter_inout_alloc();
    if (!inputs || !outputs) {
        avfilter_inout_free(&inputs); avfilter_inout_free(&outputs); return AVERROR(ENOMEM);
    }
    outputs->name = av_strdup("in"); outputs->filter_ctx = audio->source;
    inputs->name = av_strdup("out"); inputs->filter_ctx = audio->sink;
    result = avfilter_graph_parse_ptr(audio->graph, filters, &inputs, &outputs, NULL);
    avfilter_inout_free(&inputs); avfilter_inout_free(&outputs);
    return result < 0 ? result : avfilter_graph_config(audio->graph, NULL);
}

int hg_audio_open(HgAudio **out, const uint8_t *extra, int extra_size,
                  int input_rate, int output_rate, int channels, double rate) {
    if (!out || extra_size <= 0 || !extra || input_rate <= 0 || output_rate <= 0 ||
        channels <= 0 || channels > 8 || tempo_stages(rate) < 0) return AVERROR(EINVAL);
    *out = NULL;
    const AVCodec *codec = avcodec_find_decoder(AV_CODEC_ID_AAC);
    if (!codec) return AVERROR_DECODER_NOT_FOUND;
    HgAudio *audio = av_mallocz(sizeof(*audio));
    if (!audio) return AVERROR(ENOMEM);
    audio->output_rate = output_rate;
    audio->channels = channels;
    audio->rate = rate;
    av_channel_layout_default(&audio->output_layout, channels);
    audio->codec = avcodec_alloc_context3(codec);
    if (!audio->codec) { hg_audio_free(&audio); return AVERROR(ENOMEM); }
    audio->codec->pkt_timebase = (AVRational){ 1, input_rate };
    audio->codec->extradata = av_mallocz((size_t)extra_size + AV_INPUT_BUFFER_PADDING_SIZE);
    if (!audio->codec->extradata) { hg_audio_free(&audio); return AVERROR(ENOMEM); }
    memcpy(audio->codec->extradata, extra, extra_size);
    audio->codec->extradata_size = extra_size;
    int result = avcodec_open2(audio->codec, codec, NULL);
    if (result >= 0) result = create_filter(audio);
    if (result < 0) { hg_audio_free(&audio); return result; }
    *out = audio;
    return 0;
}

void hg_audio_free(HgAudio **audio) {
    if (!audio || !*audio) return;
    av_frame_free(&(*audio)->current);
    av_frame_free(&(*audio)->pending);
    avcodec_free_context(&(*audio)->codec);
    swr_free(&(*audio)->resampler);
    avfilter_graph_free(&(*audio)->graph);
    av_channel_layout_uninit(&(*audio)->output_layout);
    av_channel_layout_uninit(&(*audio)->input_layout);
    av_freep(audio);
}

int hg_audio_send(HgAudio *audio, const uint8_t *data, int size, int64_t pts) {
    if (size < 0) return AVERROR(EINVAL);
    AVPacket *packet = NULL;
    if (data) {
        packet = av_packet_alloc();
        if (!packet) return AVERROR(ENOMEM);
        int result = av_new_packet(packet, size);
        if (result < 0) { av_packet_free(&packet); return result; }
        memcpy(packet->data, data, size);
        packet->pts = packet->dts = pts;
    }
    int result = avcodec_send_packet(audio->codec, packet);
    av_packet_free(&packet);
    return result == AVERROR(EAGAIN) ? 0 : result < 0 ? result : 1;
}

static int configure_resampler(HgAudio *audio, AVFrame *frame) {
    if (audio->resampler) {
        if (frame->sample_rate != audio->input_rate || frame->format != audio->input_format ||
            av_channel_layout_compare(&frame->ch_layout, &audio->input_layout)) return AVERROR_INPUT_CHANGED;
        return 0;
    }
    audio->input_rate = frame->sample_rate;
    audio->input_format = frame->format;
    int result = av_channel_layout_copy(&audio->input_layout, &frame->ch_layout);
    if (result < 0) return result;
    result = swr_alloc_set_opts2(&audio->resampler, &audio->output_layout, AV_SAMPLE_FMT_FLT,
                                 audio->output_rate, &frame->ch_layout, frame->format,
                                 frame->sample_rate, 0, NULL);
    return result < 0 ? result : swr_init(audio->resampler);
}

static int resample(HgAudio *audio, const AVFrame *input, AVFrame **output) {
    *output = NULL;
    if (!audio->resampler) return 0;
    int samples = input ? input->nb_samples : 0;
    int64_t delayed = swr_get_delay(audio->resampler, audio->input_rate);
    int capacity = (int)av_rescale_rnd(delayed + samples, audio->output_rate, audio->input_rate, AV_ROUND_UP);
    if (capacity <= 0) return 0;
    AVFrame *frame = av_frame_alloc();
    if (!frame) return AVERROR(ENOMEM);
    frame->format = AV_SAMPLE_FMT_FLT;
    frame->sample_rate = audio->output_rate;
    frame->nb_samples = capacity;
    int result = av_channel_layout_copy(&frame->ch_layout, &audio->output_layout);
    if (result >= 0) result = av_frame_get_buffer(frame, 0);
    if (result >= 0) result = swr_convert(audio->resampler, frame->data, capacity,
                                         input ? (const uint8_t **)input->extended_data : NULL, samples);
    if (result <= 0) { av_frame_free(&frame); return result; }
    frame->nb_samples = result;
    frame->pts = !input || input->pts == AV_NOPTS_VALUE ? AV_NOPTS_VALUE : av_rescale_q(input->pts,
                      audio->codec->pkt_timebase, (AVRational){1, audio->output_rate}) -
                      av_rescale(delayed, audio->output_rate, audio->input_rate);
    *output = frame;
    return 1;
}

static int return_pcm(HgAudio *audio, AVFrame *frame, HgPcmInfo *info) {
    if (frame->format != AV_SAMPLE_FMT_FLT || frame->ch_layout.nb_channels != audio->channels) {
        av_frame_free(&frame); return AVERROR(EINVAL);
    }
    audio->current = frame;
    info->samples = (const float *)frame->data[0];
    info->frames = frame->nb_samples;
    info->channels = audio->channels;
    info->sample_rate = audio->output_rate;
    info->pts = frame->pts;
    info->media_start_secs = audio->has_anchor ? audio->anchor_secs +
        (double)audio->output_frames * audio->rate / audio->output_rate : NAN;
    info->media_duration_secs = (double)frame->nb_samples * audio->rate / audio->output_rate;
    audio->output_frames += frame->nb_samples;
    return 1;
}

static int next_segment(HgAudio *audio) {
    swr_free(&audio->resampler);
    av_channel_layout_uninit(&audio->input_layout);
    avfilter_graph_free(&audio->graph);
    audio->source = audio->sink = NULL;
    audio->segment_draining = audio->resampler_eof = audio->graph_eof = 0;
    audio->has_anchor = 0;
    audio->output_frames = 0;
    return create_filter(audio);
}

int hg_audio_reset(HgAudio *audio, double rate) {
    if (tempo_stages(rate) < 0) return AVERROR(EINVAL);
    avcodec_flush_buffers(audio->codec);
    av_frame_free(&audio->current);
    av_frame_free(&audio->pending);
    audio->decoder_eof = 0;
    audio->rate = rate;
    return next_segment(audio);
}

int hg_audio_receive(HgAudio *audio, HgPcmInfo *info) {
    av_frame_free(&audio->current);
    for (;;) {
        if (audio->sink) {
            AVFrame *filtered = av_frame_alloc();
            if (!filtered) return AVERROR(ENOMEM);
            int result = av_buffersink_get_frame(audio->sink, filtered);
            if (result >= 0) return return_pcm(audio, filtered, info);
            av_frame_free(&filtered);
            if (result == AVERROR_EOF) {
                if (!audio->pending) return 0;
                result = next_segment(audio);
                if (result < 0) return result;
                continue;
            }
            if (result != AVERROR(EAGAIN)) return result;
        }
        AVFrame *converted = NULL;
        int result;
        if (!audio->decoder_eof && !audio->segment_draining) {
            AVFrame *input = audio->pending;
            audio->pending = NULL;
            if (input) result = 0;
            else {
                input = av_frame_alloc();
                if (!input) return AVERROR(ENOMEM);
                result = avcodec_receive_frame(audio->codec, input);
            }
            if (result == AVERROR(EAGAIN)) { av_frame_free(&input); return 0; }
            if (result == AVERROR_EOF) audio->decoder_eof = 1;
            else if (result >= 0) {
                double timestamp = input->pts == AV_NOPTS_VALUE ? NAN :
                    input->pts * av_q2d(audio->codec->pkt_timebase);
                const double rounding_tolerance_samples = 2.0;
                if (audio->has_anchor && isfinite(timestamp) &&
                    fabs(timestamp - audio->expected_secs) > rounding_tolerance_samples / input->sample_rate) {
                    audio->pending = input;
                    input = NULL;
                    audio->segment_draining = 1;
                } else {
                    if (!audio->has_anchor && isfinite(timestamp)) {
                        audio->anchor_secs = timestamp;
                        audio->has_anchor = 1;
                    }
                    audio->expected_secs = timestamp + (double)input->nb_samples / input->sample_rate;
                    result = configure_resampler(audio, input);
                    if (result >= 0) result = resample(audio, input, &converted);
                }
            }
            av_frame_free(&input);
            if (result < 0 && result != AVERROR_EOF) return result;
        }
        if ((audio->decoder_eof || audio->segment_draining) && !converted && !audio->resampler_eof) {
            result = resample(audio, NULL, &converted);
            if (result < 0) return result;
            if (!converted) audio->resampler_eof = 1;
        }
        if (converted) {
            if (!audio->source) return return_pcm(audio, converted, info);
            result = av_buffersrc_add_frame_flags(audio->source, converted, 0);
            av_frame_free(&converted);
            if (result < 0) return result;
        } else if (audio->resampler_eof) {
            if (!audio->source || audio->graph_eof) {
                if (!audio->pending) return 0;
                result = next_segment(audio);
                if (result < 0) return result;
                continue;
            }
            result = av_buffersrc_add_frame_flags(audio->source, NULL, 0);
            if (result < 0) return result;
            audio->graph_eof = 1;
        }
    }
}
