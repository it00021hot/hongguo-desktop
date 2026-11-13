# NSH media sources

Local files and decrypted online streams expose fixed original MP4 lengths through scoped loopback Range URLs. Requests must match Host and carry no Origin. Dropping a lease revokes readers and its process-local frame registration.

For `PlayResponse.direct`, `DirectSession` indexes and decodes the original source on one cancellable worker. The platform receives an opaque frame registration and an audio-only URL: each response masks the original video track headers without changing sample offsets. BGRA pixels retain the shared decoder's color conversion; native media supplies AAC audio, duration and the playback clock. H.264 local files use the ordinary native URL.

The route never creates compatibility output, growing MP4, HLS playlists or transcode workers. GPU completion and native audio seek completion both gate direct seek confirmation.
