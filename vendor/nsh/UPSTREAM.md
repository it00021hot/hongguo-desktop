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
