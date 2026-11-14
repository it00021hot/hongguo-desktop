# 后端测试样本

| 文件                     | 是什么                                                                | 用途                                                                                                   | 来源                                   |
| ------------------------ | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ | -------------------------------------- |
| `heic-cover-sample.heic` | 25 KB：单 `hvc1` item（`hvcC` / `ispe` / `iloc`）的 HEIC 封面原样字节 | `hongguo_media::heif::tests::decodes_real_cdn_sample_to_jpeg`——解析 → 软解 → JPEG，产物必须是合法 JPEG | 真实 CDN 样本（`fqnovelpic` 封面原样） |

由来：2026-10-11 抽 `hongguo-media` crate 时从 `src-tauri/tests/fixtures/` 随 `heif.rs` 一并迁出（`heif.rs` 的测试用 `CARGO_MANIFEST_DIR/tests/fixtures/` 定位样本，跟着代码走才对得上）。

> 注册的 TT-Encrypt V5 样本（`register_req_real.bin` / `py_enc_accepted.hex`）已随 `domain/api` 迁到 `crates/hongguo-api/tests/fixtures/`，见那里的 README。

## 合成码流样本（2026-11-15 自 `hongguo-softdec/tests/fixtures` 迁入）

这些文件是用 ffmpeg 7.1.1（libx265 / libx264）从合成测试图案编出来的**标准码流**，每个只有几 KB 到几十 KB。它们不含任何真实剧集内容，也**不是 ByteVC1**——用来验证容器解析、时间轴、关键帧跳转、帧打包，以及解核对标准 HEVC 的逐字节正确性；另供 `hongguo-nsh-video-macos/examples/pixel_surface` 实播夹具使用。

| 文件                              | 内容                                                                                       | 覆盖点                                   |
| --------------------------------- | ------------------------------------------------------------------------------------------ | ---------------------------------------- |
| `hevc-main-136x76-aac.mp4`        | HEVC Main 8-bit，136×76，24fps，48 帧，关键帧间隔 12，带 B 帧；AAC-LC；`hvc1`；`moov` 在尾 | 裁剪窗、B 帧重排、跳转、音轨视图         |
| `hevc-main10-128x72-hev1-aac.mp4` | HEVC Main 10，128×72，24 帧，带 B 帧；AAC-LC；`hev1`；`moov` 在头                          | 10-bit 截断到 8-bit、`hev1`、`moov` 位置 |
| `hevc-main-64x48-no-audio.mp4`    | HEVC Main，无音轨                                                                          | 无音轨时的拒绝                           |
| `h264-64x48-aac.mp4`              | H.264 High；AAC-LC                                                                         | 不支持的编码按名字拒绝                   |

### 重新生成

```bash
SINE=(-f lavfi -i "sine=frequency=440:sample_rate=44100")
ffmpeg -f lavfi -i "testsrc2=size=136x76:rate=24" "${SINE[@]}" -t 2 \
  -c:v libx265 -preset medium -x265-params "keyint=12:min-keyint=12:bframes=3" \
  -pix_fmt yuv420p -tag:v hvc1 -c:a aac -b:a 24k -shortest -bitexact hevc-main-136x76-aac.mp4
ffmpeg -f lavfi -i "testsrc2=size=128x72:rate=24" "${SINE[@]}" -t 1 \
  -c:v libx265 -preset medium -x265-params "keyint=12:min-keyint=12:bframes=3" \
  -pix_fmt yuv420p10le -tag:v hev1 -c:a aac -b:a 24k -shortest -movflags +faststart -bitexact \
  hevc-main10-128x72-hev1-aac.mp4
ffmpeg -f lavfi -i "testsrc2=size=64x48:rate=24" -t 0.5 \
  -c:v libx265 -preset medium -pix_fmt yuv420p -tag:v hvc1 -an -bitexact hevc-main-64x48-no-audio.mp4
ffmpeg -f lavfi -i "testsrc2=size=64x48:rate=24" "${SINE[@]}" -t 0.5 \
  -c:v libx264 -preset medium -pix_fmt yuv420p -c:a aac -b:a 24k -shortest -bitexact h264-64x48-aac.mp4
```

换了编码器版本，产物字节会变，解码解封面的像素指纹要跟着重算（相关断言在 `hongguo-hevc` / `hongguo-media` 的解码与打包测试里）。
