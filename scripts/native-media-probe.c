#include <libavcodec/avcodec.h>
#include <libavfilter/avfilter.h>
#include <libavutil/avutil.h>
#include <libavutil/version.h>
#include <libswresample/swresample.h>
#include <stdio.h>
#include <string.h>

int main(void) {
  const enum AVCodecID codecs[] = {AV_CODEC_ID_H264, AV_CODEC_ID_HEVC,
                                   AV_CODEC_ID_AAC};
  const char *filters[] = {"abuffer", "abuffersink", "atempo",
                           "aformat", "aresample",   "anull"};
  for (unsigned i = 0; i < sizeof(codecs) / sizeof(codecs[0]); ++i) {
    if (!avcodec_find_decoder(codecs[i]) || avcodec_find_encoder(codecs[i]))
      return 1;
    AVCodecParserContext *parser = av_parser_init(codecs[i]);
    if (!parser)
      return 2;
    av_parser_close(parser);
  }
  for (unsigned i = 0; i < sizeof(filters) / sizeof(filters[0]); ++i) {
    if (!avfilter_get_by_name(filters[i]))
      return 3;
  }
  if (!strstr(avcodec_license(), "LGPL"))
    return 4;
  if (LIBAVCODEC_VERSION_MAJOR != 63 || LIBAVUTIL_VERSION_MAJOR != 61)
    return 5;
  AVFilterGraph *graph = avfilter_graph_alloc();
  if (!graph)
    return 6;
  int result = avfilter_graph_parse_ptr(
      graph,
      "abuffer=sample_rate=48000:sample_fmt=fltp:channel_layout=stereo,"
      "atempo=1.7320508075688772,atempo=1.7320508075688772,abuffersink",
      NULL, NULL, NULL);
  if (result >= 0)
    result = avfilter_graph_config(graph, NULL);
  avfilter_graph_free(&graph);
  if (result < 0)
    return 7;
  printf(
      "FFmpeg native probe: avcodec=%u avutil=%u avfilter=%u swresample=%u\n",
      avcodec_version(), avutil_version(), avfilter_version(),
      swresample_version());
  return 0;
}
