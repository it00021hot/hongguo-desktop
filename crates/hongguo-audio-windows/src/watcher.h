#ifndef HONGGUO_AUDIO_WATCHER_H
#define HONGGUO_AUDIO_WATCHER_H
#include <mmdeviceapi.h>
#include <stdbool.h>
typedef struct HgWatcher HgWatcher;
HRESULT hg_watcher_create(IMMDeviceEnumerator *enumerator, IMMDevice *device,
                          HgWatcher **result);
bool hg_watcher_changed(HgWatcher *watcher);
void hg_watcher_close(IMMDeviceEnumerator *enumerator, HgWatcher *watcher);
#endif
