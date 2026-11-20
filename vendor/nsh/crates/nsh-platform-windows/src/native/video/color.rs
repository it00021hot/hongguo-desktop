use windows::{
    Win32::Graphics::Direct2D::{
        CLSID_D2D1ColorManagement, CLSID_D2D1ColorMatrix,
        Common::{D2D_RECT_F, D2D1_COMPOSITE_MODE_SOURCE_OVER},
        D2D1_COLOR_SPACE_SCRGB, D2D1_COLOR_SPACE_SRGB,
        D2D1_COLORMANAGEMENT_PROP_DESTINATION_COLOR_CONTEXT,
        D2D1_COLORMANAGEMENT_PROP_SOURCE_COLOR_CONTEXT, D2D1_COLORMATRIX_PROP_COLOR_MATRIX,
        D2D1_INTERPOLATION_MODE_LINEAR, D2D1_PROPERTY_TYPE_COLOR_CONTEXT,
        D2D1_PROPERTY_TYPE_MATRIX_5X4, ID2D1Bitmap1, ID2D1DeviceContext, ID2D1Effect,
    },
    core::{Interface, Result},
};
use windows_numerics::Matrix3x2;

pub(super) struct VideoColor {
    white: ID2D1Effect,
}

impl VideoColor {
    pub(super) fn new(dc: &ID2D1DeviceContext, bitmap: &ID2D1Bitmap1) -> Result<Self> {
        // SAFETY: effects and color contexts belong to the bitmap's live device.
        unsafe {
            let mapping = dc.CreateEffect(&CLSID_D2D1ColorManagement)?;
            let srgb = dc.CreateColorContext(D2D1_COLOR_SPACE_SRGB, None)?;
            let scrgb = dc.CreateColorContext(D2D1_COLOR_SPACE_SCRGB, None)?;
            for (property, context) in [
                (D2D1_COLORMANAGEMENT_PROP_SOURCE_COLOR_CONTEXT, srgb),
                (D2D1_COLORMANAGEMENT_PROP_DESTINATION_COLOR_CONTEXT, scrgb),
            ] {
                mapping.SetValue(
                    property.0 as u32,
                    D2D1_PROPERTY_TYPE_COLOR_CONTEXT,
                    &(context.as_raw() as usize).to_ne_bytes(),
                )?;
            }
            mapping.SetInput(0, bitmap, false);
            let white = dc.CreateEffect(&CLSID_D2D1ColorMatrix)?;
            white.SetInput(0, &mapping.GetOutput()?, false);
            Ok(Self { white })
        }
    }

    pub(super) fn draw(
        &self,
        dc: &ID2D1DeviceContext,
        destination: D2D_RECT_F,
        white_scale: f32,
    ) -> Result<()> {
        let matrix = [
            white_scale,
            0.0,
            0.0,
            0.0,
            0.0,
            white_scale,
            0.0,
            0.0,
            0.0,
            0.0,
            white_scale,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        // SAFETY: properties are fixed-size native floats; the context is in a draw pass.
        unsafe {
            self.white.SetValue(
                D2D1_COLORMATRIX_PROP_COLOR_MATRIX.0 as u32,
                D2D1_PROPERTY_TYPE_MATRIX_5X4,
                matrix.map(f32::to_ne_bytes).as_flattened(),
            )?;
            let image = self.white.GetOutput()?;
            // Direct2D resolves the effect's image bounds at the draw context's current DPI.
            let bounds = dc.GetImageLocalBounds(&image)?;
            let fit = Matrix3x2::scale(
                (destination.right - destination.left) / (bounds.right - bounds.left),
                (destination.bottom - destination.top) / (bounds.bottom - bounds.top),
            ) * Matrix3x2::translation(destination.left, destination.top);
            let mut world = Matrix3x2::identity();
            dc.GetTransform(&mut world);
            dc.SetTransform(&(fit * world));
            dc.DrawImage(
                &image,
                None,
                None,
                D2D1_INTERPOLATION_MODE_LINEAR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );
            dc.SetTransform(&world);
        }
        Ok(())
    }
}
