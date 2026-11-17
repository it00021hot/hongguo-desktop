use hongguo_common::{AppError, AppResult};
use hongguo_signer::medusa_v5::{Field13, License, RequestSigner};

pub(super) fn load() -> AppResult<RequestSigner> {
    let license = License::decode(include_str!("../../../../assets/metasec/v5/license.txt"))
        .ok_or(AppError::PlaybackMaterials(
            "invalid bundled MetaSec license",
        ))?;
    let image =
        Field13::from_image(include_bytes!("../../../../assets/metasec/v5/image.bin").to_vec())
            .map_err(|_| AppError::PlaybackMaterials("unsupported bundled MetaSec image"))?;
    Ok(RequestSigner::new(image, license))
}
