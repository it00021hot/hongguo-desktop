use hongguo_media_types::{ColorMetadata, ContentLightMetadata, MasteringDisplayMetadata};
use hongguo_vvdec_sys as ffi;

pub(super) unsafe fn read(
    decoder: *mut ffi::vvdecDecoder,
    frame: *mut ffi::vvdecFrame,
) -> ColorMetadata {
    let mut color = ColorMetadata::default();
    if let Some(attributes) = unsafe { (*frame).picAttributes.as_ref() }
        && let Some(vui) = unsafe { attributes.vui.as_ref() }
    {
        color = from_vui(vui);
    }
    let mastering = unsafe {
        ffi::vvdec_find_frame_sei(
            decoder,
            ffi::vvdecSEIPayloadType_VVDEC_MASTERING_DISPLAY_COLOUR_VOLUME,
            frame,
        )
    };
    if let Some(sei) = unsafe { mastering.as_ref() }
        && let Some(value) = unsafe {
            sei.payload
                .cast::<ffi::vvdecSEIMasteringDisplayColourVolume>()
                .as_ref()
        }
    {
        color.mastering_display = Some(MasteringDisplayMetadata {
            primaries: value.primaries,
            white_point: value.whitePoint,
            max_luminance: value.maxLuminance,
            min_luminance: value.minLuminance,
        });
    }
    let light = unsafe {
        ffi::vvdec_find_frame_sei(
            decoder,
            ffi::vvdecSEIPayloadType_VVDEC_CONTENT_LIGHT_LEVEL_INFO,
            frame,
        )
    };
    if let Some(sei) = unsafe { light.as_ref() }
        && let Some(value) = unsafe {
            sei.payload
                .cast::<ffi::vvdecSEIContentLightLevelInfo>()
                .as_ref()
        }
    {
        color.content_light = Some(ContentLightMetadata {
            max_content_light_level: value.maxContentLightLevel,
            max_pic_average_light_level: value.maxPicAverageLightLevel,
        });
    }
    color
}

fn from_vui(vui: &ffi::vvdecVui) -> ColorMetadata {
    let byte = |value| u8::try_from(value).ok();
    let chroma_location = if vui.progressiveSourceFlag && !vui.interlacedSourceFlag {
        vui.chromaSampleLocType
    } else {
        vui.chromaSampleLocTypeTopField
    };
    ColorMetadata {
        primaries: vui
            .colourDescriptionPresentFlag
            .then(|| byte(vui.colourPrimaries))
            .flatten(),
        transfer: vui
            .colourDescriptionPresentFlag
            .then(|| byte(vui.transferCharacteristics))
            .flatten(),
        matrix: vui
            .colourDescriptionPresentFlag
            .then(|| byte(vui.matrixCoefficients))
            .flatten(),
        full_range: vui
            .videoSignalTypePresentFlag
            .then_some(vui.videoFullRangeFlag),
        chroma_location: vui
            .chromaLocInfoPresentFlag
            .then(|| {
                byte(chroma_location)
                    .filter(|value| *value <= 5)
                    .map(|value| value + 1)
            })
            .flatten(),
        sample_aspect_ratio: vui
            .aspectRatioInfoPresentFlag
            .then(|| aspect_ratio(vui))
            .flatten(),
        ..Default::default()
    }
}

fn aspect_ratio(vui: &ffi::vvdecVui) -> Option<(u32, u32)> {
    const RATIOS: [(u32, u32); 17] = [
        (0, 0),
        (1, 1),
        (12, 11),
        (10, 11),
        (16, 11),
        (40, 33),
        (24, 11),
        (20, 11),
        (32, 11),
        (80, 33),
        (18, 11),
        (15, 11),
        (64, 33),
        (160, 99),
        (4, 3),
        (3, 2),
        (2, 1),
    ];
    match vui.aspectRatioIdc {
        1..=16 => Some(RATIOS[vui.aspectRatioIdc as usize]),
        255 if vui.sarWidth > 0 && vui.sarHeight > 0 => {
            Some((vui.sarWidth as u32, vui.sarHeight as u32))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vui_preserves_hdr_color_range_location_and_sar() {
        let vui = ffi::vvdecVui {
            colourDescriptionPresentFlag: true,
            colourPrimaries: 9,
            transferCharacteristics: 16,
            matrixCoefficients: 9,
            videoSignalTypePresentFlag: true,
            videoFullRangeFlag: true,
            chromaLocInfoPresentFlag: true,
            progressiveSourceFlag: true,
            chromaSampleLocType: 2,
            aspectRatioInfoPresentFlag: true,
            aspectRatioIdc: 255,
            sarWidth: 4,
            sarHeight: 3,
            ..unsafe { std::mem::zeroed() }
        };
        let color = from_vui(&vui);
        assert_eq!(
            (color.primaries, color.transfer, color.matrix),
            (Some(9), Some(16), Some(9))
        );
        assert_eq!(color.full_range, Some(true));
        // VVC bitstream location 2 is top-left; the shared metadata uses the
        // FFmpeg enum with 0 reserved for unspecified, hence value 3.
        assert_eq!(color.chroma_location, Some(3));
        assert_eq!(color.sample_aspect_ratio, Some((4, 3)));
        assert_eq!(
            from_vui(&unsafe { std::mem::zeroed() }),
            ColorMetadata::default()
        );
    }
}
