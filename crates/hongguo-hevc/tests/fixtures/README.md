The synthetic full-range HEVC sample was moved from the removed codec FFI tests.
Its decoded I420 bytes match the existing 136×76 Main sample, while its VUI marks
full range. FNV-1a64: `e3b2ac820a1793b7` (744,192 bytes), independently checked with
a reference decoder. Full range does not imply this sample reaches code 255.
Tests consume the fixture and fixed golden; no external decoder is required.
