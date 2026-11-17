# MetaSec v5 固定材料 / Pinned material

这些是从官方 APK 一次性提取、校验后保存的应用级材料，不包含用户设备会话。构建仅校验本地文件并内嵌，构建和运行均不下载 APK 或片段。不保存解密后的签名密钥。

These application-level materials were extracted and verified once from the official APK. They contain no user device sessions or plaintext signing keys. Builds validate and embed these local files; neither builds nor runtime fetch APK material.

来源 / Source: [官方下载落地页](https://applink.novelquickapp.com/FK6cu) 的 `novelread_78105987a_v12267_74532_46e0_1794506013.apk`，版本 7.4.5.32 / 74532。

APK SHA-256: `b1ee6c1af73618adc69c92aca645b77b25dc3b59406efee68f488d3d1a6e8217`。

| 文件 / File | 提取位置 / Extraction | 字节 / Bytes | SHA-256 |
| --- | --- | --- | --- |
| `license.txt` | `classes26.dex`，解压后偏移 / unpacked offset 1918408 | 304 | `584a01183f1e6783326bff49d4f1319f33fcce678afb93ca0dc8b60a56b65855` |
| `image.bin` | `lib/arm64-v8a/libmetasec_ml.so`，解压内容 / unpacked content | 4501136 | `c6831ab265a0cb76dacfa3b7041e9ea0f706490106a74e12f39058671cc14fe3` |

两份内容与项目原先使用的 7.4.3.32 材料逐字节相同；设备版本与签名契约不变。原生库作为受限字节码解释器的只读输入，不作为宿主动态库执行。来源内容归原权利人所有。

Both files are byte-identical to the previously verified 7.4.3.32 material. Device identity and signing contracts remain unchanged. The image is read-only input for the bounded interpreter, never loaded as a host native library. Source contents remain the property of their original rights holders.
