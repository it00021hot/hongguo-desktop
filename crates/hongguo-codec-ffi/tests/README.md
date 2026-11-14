# Codec fixtures

The standard Main/Main10/H.264 clips used by `decode.rs` are synthetic fixtures
from `hongguo-softdec/tests/fixtures`, not Hongguo content.

`fixtures-hevc-full-range.mp4` is derived from the synthetic Main clip without
re-encoding. The HEVC VUI and MP4 color metadata explicitly mark full range, while
keeping the same decoded code values. This tests hardware range preservation and
detects an unwanted limited-range conversion:

```sh
ffmpeg -v error -i crates/hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4 \
  -map 0:v:0 -c:v copy -bsf:v hevc_metadata=video_full_range_flag=1 \
  -color_range pc -movflags +write_colr -an \
  crates/hongguo-codec-ffi/tests/fixtures-hevc-full-range.mp4
```

FFmpeg CLI is only a development fixture generator. Playback and tests decode
through the bundled libraries; the customer installs no external FFmpeg program.
