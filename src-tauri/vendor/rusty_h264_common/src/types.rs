//! Shared codec types: profiles, chroma format, and the raw YUV frame container.

#[allow(unused_imports)]
use alloc::vec;
use alloc::vec::Vec;

/// H.264 profile. The encoder targets Constrained Baseline; the rest are named
/// for parsing/identification only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Constrained Baseline (`profile_idc = 66` with `constraint_set1_flag`).
    ConstrainedBaseline,
    /// Baseline (`profile_idc = 66`).
    Baseline,
    /// Main (`profile_idc = 77`).
    Main,
    /// High (`profile_idc = 100`).
    High,
    /// Any other `profile_idc`.
    Other(u8),
}

impl Profile {
    /// The `profile_idc` byte written into the SPS.
    pub fn profile_idc(self) -> u8 {
        match self {
            Profile::ConstrainedBaseline | Profile::Baseline => 66,
            Profile::Main => 77,
            Profile::High => 100,
            Profile::Other(v) => v,
        }
    }
}

/// Chroma subsampling. The encoder supports 4:2:0 only for now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromaFormat {
    /// Monochrome (`chroma_format_idc = 0`).
    Monochrome,
    /// 4:2:0 (`chroma_format_idc = 1`).
    Yuv420,
}

impl ChromaFormat {
    /// `chroma_format_idc`.
    pub fn idc(self) -> u8 {
        match self {
            ChromaFormat::Monochrome => 0,
            ChromaFormat::Yuv420 => 1,
        }
    }
}

/// A raw planar YUV 4:2:0 frame (8-bit). Plane strides equal their widths;
/// chroma planes are half-resolution in each dimension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YuvFrame {
    /// Luma width in pixels.
    pub width: usize,
    /// Luma height in pixels.
    pub height: usize,
    /// Y plane, `width * height` bytes.
    pub y: Vec<u8>,
    /// Cb plane, `(width/2) * (height/2)` bytes.
    pub u: Vec<u8>,
    /// Cr plane, `(width/2) * (height/2)` bytes.
    pub v: Vec<u8>,
}

/// A **borrowed** planar 4:2:0 frame: three plane slices with row strides.
///
/// This is the encoder's input on a chip, where the camera's DMA buffer is the
/// frame and copying it into three `Vec<u8>` (a [`YuvFrame`]) would be 115 KB
/// of traffic per QVGA picture. [`YuvFrame::as_planes`] gives the same view of
/// an owned frame, so both feed the same coder.
///
/// A view is *tight* when every stride equals its plane width; the coder reads
/// tight planes directly. A padded view (stride > width) is gathered into an
/// encoder-owned scratch frame first — one copy, reused, never reallocated.
#[derive(Clone, Copy, Debug)]
pub struct YuvPlanes<'a> {
    /// Luma width in pixels (even).
    pub width: usize,
    /// Luma height in pixels (even).
    pub height: usize,
    /// Y plane, at least `stride_y * (height - 1) + width` bytes.
    pub y: &'a [u8],
    /// Cb plane, at least `stride_c * (height/2 - 1) + width/2` bytes.
    pub u: &'a [u8],
    /// Cr plane, like `u`.
    pub v: &'a [u8],
    /// Bytes between the starts of consecutive luma rows.
    pub stride_y: usize,
    /// Bytes between the starts of consecutive chroma rows.
    pub stride_c: usize,
}

impl<'a> YuvPlanes<'a> {
    /// Borrow three planes with explicit strides. `None` if a dimension is
    /// zero or odd, a stride is shorter than its row, or a plane is too short
    /// for the geometry it claims — better to refuse than to read out of
    /// bounds mid-frame.
    pub fn new(
        width: usize,
        height: usize,
        y: &'a [u8],
        u: &'a [u8],
        v: &'a [u8],
        stride_y: usize,
        stride_c: usize,
    ) -> Option<Self> {
        if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
            return None;
        }
        let (cw, ch) = (width / 2, height / 2);
        if stride_y < width || stride_c < cw {
            return None;
        }
        if y.len() < stride_y * (height - 1) + width
            || u.len() < stride_c * (ch - 1) + cw
            || v.len() < stride_c * (ch - 1) + cw
        {
            return None;
        }
        Some(Self {
            width,
            height,
            y,
            u,
            v,
            stride_y,
            stride_c,
        })
    }

    /// Borrow three tightly packed planes (stride == width).
    pub fn tight(
        width: usize,
        height: usize,
        y: &'a [u8],
        u: &'a [u8],
        v: &'a [u8],
    ) -> Option<Self> {
        Self::new(width, height, y, u, v, width, width / 2)
    }

    /// Chroma plane width.
    pub fn chroma_width(&self) -> usize {
        self.width / 2
    }

    /// Chroma plane height.
    pub fn chroma_height(&self) -> usize {
        self.height / 2
    }

    /// Every stride equals its plane width, so rows are contiguous.
    pub fn is_tight(&self) -> bool {
        self.stride_y == self.width && self.stride_c == self.chroma_width()
    }

    /// The coder's precondition: tight, with planes of exactly the packed size
    /// (what [`YuvFrame::is_valid`] checks for an owned frame).
    pub fn is_valid(&self) -> bool {
        self.is_tight()
            && self.y.len() == self.width * self.height
            && self.u.len() == self.chroma_width() * self.chroma_height()
            && self.v.len() == self.chroma_width() * self.chroma_height()
    }

    /// Gather the view into an owned, tightly packed frame.
    pub fn to_frame(&self) -> YuvFrame {
        let mut f = YuvFrame::black(self.width, self.height);
        self.copy_into(&mut f);
        f
    }

    /// Gather the view into `dst`, resizing it only if the geometry differs
    /// (so a reused scratch frame never reallocates).
    pub fn copy_into(&self, dst: &mut YuvFrame) {
        if dst.width != self.width || dst.height != self.height || !dst.is_valid() {
            *dst = YuvFrame::black(self.width, self.height);
        }
        let (w, h, cw, ch) = (
            self.width,
            self.height,
            self.chroma_width(),
            self.chroma_height(),
        );
        for r in 0..h {
            dst.y[r * w..(r + 1) * w].copy_from_slice(&self.y[r * self.stride_y..][..w]);
        }
        for r in 0..ch {
            dst.u[r * cw..(r + 1) * cw].copy_from_slice(&self.u[r * self.stride_c..][..cw]);
            dst.v[r * cw..(r + 1) * cw].copy_from_slice(&self.v[r * self.stride_c..][..cw]);
        }
    }
}

impl YuvFrame {
    /// The borrowed view of this frame (tight strides).
    pub fn as_planes(&self) -> YuvPlanes<'_> {
        YuvPlanes {
            width: self.width,
            height: self.height,
            y: &self.y,
            u: &self.u,
            v: &self.v,
            stride_y: self.width,
            stride_c: self.width / 2,
        }
    }

    /// Allocates a black (Y=0, U=V=128) frame. Dimensions must be even.
    pub fn black(width: usize, height: usize) -> Self {
        assert!(width % 2 == 0 && height % 2 == 0, "dimensions must be even");
        let cw = width / 2;
        let ch = height / 2;
        Self {
            width,
            height,
            y: vec![0; width * height],
            u: vec![128; cw * ch],
            v: vec![128; cw * ch],
        }
    }

    /// Chroma plane width.
    pub fn chroma_width(&self) -> usize {
        self.width / 2
    }

    /// Chroma plane height.
    pub fn chroma_height(&self) -> usize {
        self.height / 2
    }

    /// Validates plane sizes against the dimensions.
    pub fn is_valid(&self) -> bool {
        self.y.len() == self.width * self.height
            && self.u.len() == self.chroma_width() * self.chroma_height()
            && self.v.len() == self.chroma_width() * self.chroma_height()
    }
}
