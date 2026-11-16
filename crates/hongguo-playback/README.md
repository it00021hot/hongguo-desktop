# Native Video codec extension

Ordinary formats use NSH Video. This crate supplies a bounded video session only
for dedicated HEVC/ByteVC2 decoding and explicit frame processing.

It reads original MP4 sample tables, signed presentation timestamps and edit lists;
HEVC tries the bound native device's decoder first (direct D3D11 on Windows), with
libde265 as fallback; ByteVC2 uses VVdeC. Native-precision frames retain container
color, pixel aspect ratio and rotation metadata. Queues, decode threads, memory and
presentation recovery are bounded. Seeking replaces the video epoch; metadata reads
are cancelled only when the session closes.

Audio remains in AVPlayer, MediaEngine or MediaPlayer. An audio-only MP4 view keeps
the original file size, sample offsets and edit lists. The session consumes the
native player's clock, controls volume/rate/pause and waits for both clock readiness
and first-frame presentation before playing. Rebuffering holds the latest media time.
A native clock error is reported rather than silently switching to wall time.

There is no AAC decoder, PCM ring, resampler or tempo filter here.
Silent sources and the explicit video tail use the monotonic media clock.
A frame lease and its generation protect in-flight GPU work after seek or close.
HEVC fallback retains source VUI/SEI metadata by PTS across B-picture reordering.
Hardware failures restart at the current audio-clock position in a fresh epoch;
the same failed device is not retried until it is rebuilt.

Codecs are precompiled and authenticated by the repository SDK.
See [codec build and distribution](../../docs/native-media-build.md) and
[validation boundaries](../../docs/nsh-decoder-validation.md).
