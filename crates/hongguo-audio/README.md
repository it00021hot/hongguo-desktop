# Native PCM output

`hongguo-audio` selects AUHAL/RemoteIO, event-driven WASAPI shared mode or AAudio.
It does not decode AAC, apply tempo changes or depend on Media Foundation.
The playback session supplies interleaved `f32` PCM after decoding, resampling and
tempo processing, typically 48 kHz stereo.

## Format and lifecycle

`Output::open(requested, source)` opens a stopped device and never reads the source.
The backend preserves the requested client format; unsupported formats fail.
WASAPI and Apple may use the operating system's conversion from that explicit
client format to the hardware format. AAudio verifies its actual client format.
The current WASAPI implementation supports mono and stereo.

Call `flush(epoch)` before the first start and after each seek. Flush stops output,
discards device buffers and resets the relative device counter to the given epoch.
Only reactivate the corresponding PCM ring after flush completes. `start` resumes
consumption. `pause` stops the source and preserves the device position; Apple
first lets already submitted PCM finish, bounded by a one-second device timeout.
Dropping the output stops and joins callbacks before releasing the source.

## Clock and real-time contract

`Position.device_frames` counts device-consumed frames, including underflow silence,
relative to the latest flush. It does not count decoded or merely queued PCM.
WASAPI uses `IAudioClock`; AAudio uses the monotonic stream timestamp; Apple uses
the AudioUnit's presentation host timestamp, bounded by submitted frames.
`latency_frames` reports native buffering/latency information and must not be
subtracted again from this already consumed position.

The backend calls `PcmSource::played(epoch, device_frames)` before pulling PCM and
when its owner queries the clock. The ring maps these device frames to media PTS;
silence must not advance media time. Stale-epoch reads are silenced. Source panics
are caught at the Rust/C boundary and exposed as an output error.

`read` and `played` run on native audio threads: they must not wait, allocate or
free PCM blocks. The playback ring owns bounded preallocated queues and handles
contention with immediate silence. Platform callbacks use no application locks.
WASAPI serializes device operations on its COM thread; the UI thread never runs
the render callback.

## Host integration and verification

Device disconnection or a changed default desktop output becomes an error so the
playback session can reopen the device at the media clock. AAudio reports stream
errors. Android audio focus, route/noisy broadcasts and background policy remain
host responsibilities: AAudio usage/content type does not acquire audio focus.
iOS configures a Playback/MoviePlayback session; interruption/resume and background
policy also require host lifecycle integration.

The ignored `device` test opens an independent silent output and checks real clock
progress, pause and epoch flush. It produces no audible tone and does not launch
or control the application:

```sh
cargo test -p hongguo-audio-api
cargo test -p hongguo-audio --test device -- --ignored
```

macOS device execution and iOS simulator/Android/Windows target compilation are
separate evidence. Cross compilation does not validate mobile/Windows device
timing, route changes, interruptions or actual application playback.
