# Controlled Native Shell source

Source: https://github.com/longbridge/nsh

Release: v0.5.1

Commit: 224d9a795c7ac9d10d75750de9351adac5549675

The source snapshot keeps all upstream crates, workspace metadata and licenses,
compile-time fixtures, and workspace member examples. The website and unrelated
example applications are omitted. All application NSH dependencies must resolve
to this one tree. The Cargo and Git caches are never patched in place.

Windows native components can request compositor-paced drawing through
`ComponentContext::request_frames` and report continued demand with
`NativeComponent::needs_frames`. The component host uses the same surface
`on_draw_frame` client as native Video, with a weak host reference and one
pump per component. This bridges the custom-component scheduling gap without
adding a second display clock. Remove it when upstream provides equivalent
native-component frame scheduling.

Local changes include the Windows backend's same-canvas high-precision display
composition. Native MediaEngine video maps its SDR BGRA8 frames from sRGB to
linear scRGB on the GPU before drawing on an FP16 canvas, including the current
display's SDR white scaling. The standard SDR canvas retains direct bitmap drawing.
Remove this bridge when upstream provides equivalent native-video color management.
Android additionally exposes a
conservative `SurfaceComposition.supports(View)` admission check: separate HDR
surfaces are excluded under Canvas shaders/transitions, masks, rounded ancestor clipping,
nonidentity transforms, or partial ancestor opacity. Intersecting sibling overlays
and their descendants are also checked for transforms, partial opacity and
shader/mask composition; their own rounded button clips remain permitted. Rectangular video surfaces
stay below the NSH window so sibling controls remain visible. These changes are intended to be
proposed upstream and removed when an upstream release provides the same API.

Windows screen presence subscribes to `GUID_SESSION_DISPLAY_STATUS`, as required
for interactive user-mode applications, instead of the console monitor's
`GUID_CONSOLE_DISPLAY_STATE`. Remote Desktop and Hyper-V enhanced sessions must
follow their own display state when the console display is off. Lock, disconnect,
minimize and window-cloaking lifecycle handling is retained.

On Android API 26–29, the bottom safe-area and native tab-bar insets are capped by the stable system-bar
inset. The legacy system-window inset also includes the IME during `adjustResize`;
using it as a safe area gives the native tab bar a keyboard-sized bottom padding.
The legacy system-window inset above the stable bottom inset also supplies the IME height.
Keyboard overlap is reduced by the space below the embedded view, so resized windows do not
receive duplicate keyboard padding, while fullscreen windows can use the same keyboard
environment and padding APIs as Android 11+. Remove this patch when upstream reports legacy IME overlap.

The pinned CLI supports an embedding application's `nativeHost` configuration:
`build` and `stage` are argument arrays, and `hosts` maps a target to its native
runner path (`{profile}` and `{triple}` are substituted). Builds run in the app
directory; staging runs before desktop archives or Apple signing. Android uses
the supplied dex/library directory and its optional manifest/resources/assets, while
NSH still assembles, aligns and signs APK/AAB output. `android.activity` selects
the embedding activity. Missing outputs or ABIs are fatal. This project-specific
extension is not part of upstream 0.5.1; users must build this CLI. It preserves
custom JNI components and static-codec relinking without maintaining a second
packager or production signing pipeline. `android.abis` supplies the default ABI
list and `nativeHost.displayNames` holds localized product names; UI catalogs and
platform labels are generated from the same metadata. Native-host packaging skips
the UI description build because its host is supplied independently, so relinking
recipients can repackage a replacement binary without recompiling application code.
It should be proposed upstream and
removed when the released CLI offers equivalent native-host integration.

Android platform lookup also accepts stable SDK directories whose package names
include a minor API suffix, by checking `source.properties`; preview SDKs are not
substituted for the requested stable API.

Windows packaging accepts `windows.installerLanguage`, the alphanumeric stem of an
Inno Setup bundled language file (for example, `ChineseSimplified`). It adds the
native `[Languages]` entry to the generated installer script; absent configuration
keeps Inno Setup's default language. Missing compiler translations fail compilation.

`windows.displayName` sets the installer, shortcut and uninstall-list labels while
retaining the executable, install directory and AppId. A changed shortcut label
removes the previous name's desktop and Start menu `.lnk` files during upgrade.
