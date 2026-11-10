use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use flate2::read::DeflateDecoder;
use hongguo_common::offline_qa::{qa_remove_file, qa_rename, qa_write};
use hongguo_common::{AppError, AppResult};
use hongguo_signer::medusa_v5::{Field13, License, RequestSigner};
use reqwest::header::{ACCEPT_ENCODING, CONTENT_RANGE, RANGE};
use sha2::{Digest, Sha256};

const APK_URL: &str = "https://lf9-apk.ugapk.cn/package/apk/novelread/12267_74332/novelread_78105987a_v12267_74332_b929_1793271281.apk?v=1793271287";
const APK_LENGTH: usize = 153_286_579;
pub(super) const CACHE_NAME: &str = "metasec-v5-74332.cache";
const LICENSE_OFFSET: usize = 1_936_053;
const LICENSE_LENGTH: usize = 304;
const DEX: Part<'static> = Part {
    start: 64_901_723,
    length: 3_915_212,
    unpacked: 10_417_728,
    sha256: "46526aa656dd910e06aeedbb2b23b5b846fd029ae49841587d38fcc4f9f277e6",
};
const ELF: Part<'static> = Part {
    start: 112_946_914,
    length: 1_497_823,
    unpacked: 4_501_136,
    sha256: "3839797f0a1e0ef6c604d639d0054a527b99ed1fa6aa9ecd2e80a9d87c8d133f",
};
const CACHE_LENGTH: usize = DEX.length + ELF.length;

#[derive(Clone, Copy)]
struct Part<'a> {
    start: usize,
    length: usize,
    unpacked: usize,
    sha256: &'a str,
}

pub(super) async fn load(
    client: &reqwest::Client,
    cache: Option<&Path>,
) -> AppResult<RequestSigner> {
    crate::client::refuse_external_send(APK_URL)?;
    if let Some(path) = cache {
        let path = path.to_owned();
        if let Some(signer) = tokio::task::spawn_blocking(move || read_cache(&path))
            .await
            .map_err(|_| AppError::Internal("MetaSec cache task failed".into()))?
        {
            log::debug!("[MetaSec] 已复用本机 v5 材料");
            return Ok(signer);
        }
    }

    let (mut dex, elf) = tokio::try_join!(download(client, DEX), download(client, ELF))?;
    dex.extend(elf);
    let cache = cache.map(Path::to_owned);
    tokio::task::spawn_blocking(move || {
        let signer = signer(&dex)?;
        if let Some(path) = cache
            && let Err(error) = write_cache(&path, &dex)
        {
            log::warn!("[MetaSec] 无法保存 v5 材料缓存: {error}");
        }
        log::info!("[MetaSec] 已从官方发行包验证并准备 v5 材料");
        Ok(signer)
    })
    .await
    .map_err(|_| AppError::Internal("MetaSec material task failed".into()))?
}

async fn download(client: &reqwest::Client, part: Part<'_>) -> AppResult<Vec<u8>> {
    let end = part.start + part.length - 1;
    let expected_range = format!("bytes {}-{end}/{APK_LENGTH}", part.start);
    let mut response = client
        .get(APK_URL)
        .timeout(Duration::from_secs(20))
        .header(RANGE, format!("bytes={}-{end}", part.start))
        .header(ACCEPT_ENCODING, "identity")
        .send()
        .await
        .map_err(|error| AppError::Network(error.without_url().to_string()))?;
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT
        || response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            != Some(expected_range.as_str())
    {
        return Err(AppError::Network(format!(
            "MetaSec v5 APK Range unavailable, HTTP {}",
            response.status()
        )));
    }
    let mut bytes = Vec::with_capacity(part.length);
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| AppError::Network(error.without_url().to_string()))?
    {
        if chunk.len() > part.length - bytes.len() {
            return Err(AppError::PlaybackMaterials(
                "APK range exceeded its declared size",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn unpack(bytes: &[u8], part: Part<'_>) -> AppResult<Vec<u8>> {
    if bytes.len() != part.length || hex::encode(Sha256::digest(bytes)) != part.sha256 {
        return Err(AppError::PlaybackMaterials(
            "APK fragment integrity check failed",
        ));
    }
    let mut unpacked = Vec::with_capacity(part.unpacked);
    DeflateDecoder::new(bytes)
        .take(part.unpacked as u64 + 1)
        .read_to_end(&mut unpacked)?;
    if unpacked.len() != part.unpacked {
        return Err(AppError::PlaybackMaterials("unexpected APK fragment size"));
    }
    Ok(unpacked)
}

fn signer(bytes: &[u8]) -> AppResult<RequestSigner> {
    if bytes.len() != CACHE_LENGTH {
        return Err(AppError::PlaybackMaterials("incomplete APK fragments"));
    }
    let (dex, elf) = bytes.split_at(DEX.length);
    let dex = unpack(dex, DEX)?;
    let license = dex
        .get(LICENSE_OFFSET..LICENSE_OFFSET + LICENSE_LENGTH)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .and_then(License::decode)
        .ok_or(AppError::PlaybackMaterials(
            "invalid MetaSec license in verified APK",
        ))?;
    let image = Field13::from_image(unpack(elf, ELF)?)
        .map_err(|_| AppError::PlaybackMaterials("unsupported MetaSec image in verified APK"))?;
    Ok(RequestSigner::new(image, license))
}

fn read_cache(path: &Path) -> Option<RequestSigner> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            log::warn!("[MetaSec] 无法读取 v5 材料缓存: {error}");
            return None;
        }
    };
    let mut bytes = Vec::with_capacity(CACHE_LENGTH);
    match file
        .take(CACHE_LENGTH as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(AppError::from)
        .and_then(|_| signer(&bytes))
    {
        Ok(signer) => Some(signer),
        Err(error) => {
            log::warn!("[MetaSec] v5 材料缓存校验失败，将重新获取: {error}");
            None
        }
    }
}

fn write_cache(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = path.with_extension(format!("{:016x}.tmp", rand::random::<u64>()));
    let result = qa_write(&temp, bytes).and_then(|()| qa_rename(&temp, path));
    if result.is_err() {
        let _ = qa_remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::DeflateEncoder};
    use std::io::Write;

    #[test]
    fn fragments_are_verified_before_bounded_inflation() {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"verified").unwrap();
        let bytes = encoder.finish().unwrap();
        let sha256 = hex::encode(Sha256::digest(&bytes));
        let part = Part {
            start: 0,
            length: bytes.len(),
            unpacked: 8,
            sha256: &sha256,
        };
        assert_eq!(unpack(&bytes, part).unwrap(), b"verified");
        assert!(unpack(&bytes[..bytes.len() - 1], part).is_err());
        let mut modified = bytes.clone();
        modified[0] ^= 1;
        assert!(unpack(&modified, part).is_err());
        assert!(
            unpack(
                &bytes,
                Part {
                    unpacked: 7,
                    ..part
                }
            )
            .is_err()
        );
    }
}
