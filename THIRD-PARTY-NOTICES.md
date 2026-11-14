# 第三方组件 / Third-Party Notices

本项目使用 PolyForm Noncommercial 1.0.0。第三方组件保留各自许可，并不授予额外专利权。
This project uses PolyForm Noncommercial 1.0.0. Third-party components retain their own licenses; no additional patent rights are granted.

| 组件 / Component                   | 用途 / Purpose                                      | 许可 / License                                           |
| ---------------------------------- | --------------------------------------------------- | -------------------------------------------------------- |
| NSH                                | 原生 UI / Native UI                                 | Apache-2.0（见 vendor/nsh/LICENSE-APACHE）               |
| WGPU                               | GPU 呈现与转换 / GPU rendering                      | MIT OR Apache-2.0                                        |
| Lucide SVG                         | 原生 UI 图标 / Native UI icons                      | ISC（见 crates/hongguo-nsh/src/ui/icons/LICENSE）        |
| libde265                           | HEVC 缺口解码 / HEVC fallback                       | LGPL-3.0-or-later                                        |
| VVdeC                              | ByteVC2 解码核 / Decoder core                       | 见 crates/hongguo-vvdec-sys/vendor/vvdec/LICENSE.txt     |
| rusty_h265                         | HEIF 与测试夹具解码 / HEIF and fixtures             | Apache-2.0                                               |
| oxideav-h266                       | ByteVC2 回退与测试 / Fallback and tests             | MIT                                                      |
| aes、ctr、md-5、sm3                | 解密与摘要 / Decryption and hashing                 | MIT OR Apache-2.0                                        |
| Anime4K WebGPU                     | 离线导出的增强着色器 / Exported enhancement shaders | 见 crates/hongguo-enhancement/shaders/anime4k/LICENSE.md |
| Tokio、Serde、Reqwest 等 Rust 依赖 | 运行时、序列化与网络 / Runtime, data and networking | 各 crate 的许可 / Respective crate licenses              |
| ESLint、TypeScript、Prettier       | 开发检查 / Development tooling                      | 各包的许可 / Respective package licenses                 |

客户包中的 `NativeMedia` 提供编解码库许可、对应源码、构建参数与哈希；移动端附带重新链接材料。
Customer packages include codec licenses, corresponding sources, build configurations and hashes in `NativeMedia`; mobile packages include relinking materials.
来源及重建步骤见 [原生媒体构建说明](docs/native-media-build.md)。
See [native media build instructions](docs/native-media-build.md) for provenance and rebuilding.

## 签名算法来源 / Signature Algorithm Origin

`crates/hongguo-signer` 中部分签名常量与算法移植自
[woshishiq1/drpys @ 22261ad](https://github.com/woshishiq1/drpys)，源文件为 `spider/js/红果果[短].js`。
该来源的 GPL-3.0 授权义务独立存在且不可撤销，不随项目整体的 PolyForm Noncommercial 许可改变。
Parts of the signing constants and algorithms were ported from the source above.
Its GPL-3.0 obligations remain independently applicable and are not superseded by the project license.
