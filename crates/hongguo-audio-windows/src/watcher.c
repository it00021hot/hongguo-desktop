#define COBJMACROS
#define CONST_VTABLE
#include "watcher.h"
#include <stdbool.h>
#include <stdlib.h>
#include <wchar.h>

struct HgWatcher {
  IMMNotificationClient client;
  LONG references;
  volatile LONG changed;
  LPWSTR device_id;
};

static ULONG STDMETHODCALLTYPE retain(IMMNotificationClient *client) {
  HgWatcher *watcher = (HgWatcher *)client;
  return InterlockedIncrement(&watcher->references);
}
static ULONG STDMETHODCALLTYPE release(IMMNotificationClient *client) {
  HgWatcher *watcher = (HgWatcher *)client;
  LONG references = InterlockedDecrement(&watcher->references);
  if (!references) {
    CoTaskMemFree(watcher->device_id);
    free(watcher);
  }
  return references;
}
static HRESULT STDMETHODCALLTYPE query(IMMNotificationClient *client,
                                       REFIID id, void **result) {
  if (!result)
    return E_POINTER;
  *result = NULL;
  if (!IsEqualIID(id, &IID_IUnknown) &&
      !IsEqualIID(id, &IID_IMMNotificationClient))
    return E_NOINTERFACE;
  *result = client;
  retain(client);
  return S_OK;
}
static HRESULT STDMETHODCALLTYPE state_changed(IMMNotificationClient *client,
                                               LPCWSTR id, DWORD state) {
  HgWatcher *watcher = (HgWatcher *)client;
  if (id && !wcscmp(id, watcher->device_id) && state != DEVICE_STATE_ACTIVE)
    InterlockedExchange(&watcher->changed, 1);
  return S_OK;
}
static HRESULT STDMETHODCALLTYPE added(IMMNotificationClient *client,
                                       LPCWSTR id) {
  (void)client;
  (void)id;
  return S_OK;
}
static HRESULT STDMETHODCALLTYPE removed(IMMNotificationClient *client,
                                         LPCWSTR id) {
  return state_changed(client, id, DEVICE_STATE_NOTPRESENT);
}
static HRESULT STDMETHODCALLTYPE default_changed(
    IMMNotificationClient *client, EDataFlow flow, ERole role, LPCWSTR id) {
  HgWatcher *watcher = (HgWatcher *)client;
  if (flow == eRender && role == eMultimedia &&
      (!id || wcscmp(id, watcher->device_id)))
    InterlockedExchange(&watcher->changed, 1);
  return S_OK;
}
static HRESULT STDMETHODCALLTYPE property_changed(
    IMMNotificationClient *client, LPCWSTR id, const PROPERTYKEY key) {
  (void)client;
  (void)id;
  (void)key;
  return S_OK;
}
static const IMMNotificationClientVtbl TABLE = {
    query, retain,  release,         state_changed,
    added, removed, default_changed, property_changed};

HRESULT hg_watcher_create(IMMDeviceEnumerator *enumerator, IMMDevice *device,
                          HgWatcher **result) {
  *result = NULL;
  HgWatcher *watcher = calloc(1, sizeof(HgWatcher));
  if (!watcher)
    return E_OUTOFMEMORY;
  watcher->client.lpVtbl = &TABLE;
  watcher->references = 1;
  HRESULT status = IMMDevice_GetId(device, &watcher->device_id);
  if (FAILED(status)) {
    release(&watcher->client);
    return status;
  }
  status = IMMDeviceEnumerator_RegisterEndpointNotificationCallback(
      enumerator, &watcher->client);
  if (FAILED(status)) {
    release(&watcher->client);
    return status;
  }
  *result = watcher;
  return S_OK;
}
bool hg_watcher_changed(HgWatcher *watcher) {
  return watcher && InterlockedCompareExchange(&watcher->changed, 0, 0) != 0;
}
void hg_watcher_close(IMMDeviceEnumerator *enumerator, HgWatcher *watcher) {
  if (!watcher)
    return;
  IMMDeviceEnumerator_UnregisterEndpointNotificationCallback(
      enumerator, &watcher->client);
  release(&watcher->client);
}
