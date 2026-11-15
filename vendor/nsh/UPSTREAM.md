# Controlled Native Shell source

Source: https://github.com/longbridge/nsh

Release: v0.5.1

Commit: 224d9a795c7ac9d10d75750de9351adac5549675

The source snapshot keeps all upstream crates, workspace metadata and licenses,
compile-time fixtures, and workspace member examples. The website and unrelated
example applications are omitted. All application NSH dependencies must resolve
to this one tree. The Cargo and Git caches are never patched in place.

Local changes include the Windows backend's same-canvas high-precision display
composition. Android additionally exposes a
conservative `SurfaceComposition.supports(View)` admission check: separate HDR
surfaces are excluded under Canvas shaders/transitions, masks, rounded ancestor clipping,
nonidentity transforms, or partial ancestor opacity. Intersecting sibling overlays
and their descendants are also checked for transforms, partial opacity and
shader/mask composition; their own rounded button clips remain permitted. Rectangular video surfaces
stay below the NSH window so sibling controls remain visible. These changes are intended to be
proposed upstream and removed when an upstream release provides the same API.

On Android API 26–29, the bottom safe-area and native tab-bar insets are capped by the stable system-bar
inset. The legacy system-window inset also includes the IME during `adjustResize`;
using it as a safe area gives the native tab bar a keyboard-sized bottom padding.
The system-resized viewport continues to provide the keyboard space on these APIs.

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
