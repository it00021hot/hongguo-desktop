# Controlled Native Shell source

Source: https://github.com/longbridge/nsh

Release: v0.5.0

Commit: f4d68f7f9f31eb7b3ecc8a4cbc31096d5c6238a1

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
