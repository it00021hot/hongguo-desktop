use std::sync::OnceLock;

use hongguo_signer::device::{DeviceProfile, device_proto, fingerprint_for_cdid};
use hongguo_signer::medusa_v5::{Field13, SDK_VERSION, SDK_VERSION_CODE};
use hongguo_signer::protobuf::{WireValue, read_field};

use super::sdi::{append_bytes, append_signed};

const UNAVAILABLE: i64 = -999_999;
const SDI_UNAVAILABLE_TEXT: &[u32] = &[
    1, 19, 20, 23, 28, 31, 32, 38, 39, 50, 53, 93, 94, 95, 96, 109, 110, 111, 115, 135, 2000, 2001,
];
const SDI_UNAVAILABLE_NUMBER: &[u32] = &[
    13, 29, 30, 36, 37, 41, 42, 43, 49, 51, 52, 56, 57, 58, 59, 60, 61, 62, 63, 64, 66, 67, 69, 70,
    71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 97, 98, 99, 100, 101,
    102, 103, 104, 105, 106, 113, 114, 116, 117, 118,
];

pub(super) fn sdi_payload(device: &DeviceProfile, image: &Field13, token: Option<&str>) -> Vec<u8> {
    let marker = image.unavailable_string();
    let mut bytes = Vec::new();
    append_bytes(&mut bytes, 1, &sdi_device(device, marker));
    if let Some(token) = token {
        append_bytes(&mut bytes, 2, token.as_bytes());
    }
    append_bytes(&mut bytes, 3, device.get("device_platform").as_bytes());
    append_bytes(&mut bytes, 4, SDK_VERSION.as_bytes());
    append_signed(&mut bytes, 5, i64::from(SDK_VERSION_CODE));
    append_bytes(&mut bytes, 6, device.get("aid").as_bytes());
    append_bytes(&mut bytes, 7, device.get("version_name").as_bytes());
    append_bytes(&mut bytes, 8, device.get("device_id").as_bytes());
    append_signed(&mut bytes, 11, UNAVAILABLE);
    append_signed(&mut bytes, 16, UNAVAILABLE);
    append_bytes(&mut bytes, 17, marker.as_bytes());
    bytes
}

fn sdi_device(device: &DeviceProfile, marker: &str) -> Vec<u8> {
    enum Value<'a> {
        Number(i64),
        Text(&'a str),
    }
    let text = [
        (2, device.get("device_brand")),
        (3, device.get("device_type")),
        (5, "Android"),
        (6, device.get("os_version")),
        (7, device.get("resolution")),
        (9, rom_version(device)),
        (12, "Asia/Shanghai,8"),
        (26, boot_id()),
    ];
    let numeric = [(8, device.get("dpi")), (48, device.get("os_api"))];
    let mut fields: Vec<_> = SDI_UNAVAILABLE_TEXT
        .iter()
        .map(|field| (*field, Value::Text(marker)))
        .chain(
            SDI_UNAVAILABLE_NUMBER
                .iter()
                .map(|field| (*field, Value::Number(UNAVAILABLE))),
        )
        .chain(
            text.into_iter()
                .filter(|(_, value)| !value.is_empty())
                .map(|(field, value)| (field, Value::Text(value))),
        )
        .chain(
            numeric
                .into_iter()
                .map(|(field, value)| (field, Value::Number(value.parse().unwrap_or(UNAVAILABLE)))),
        )
        .collect();
    fields.sort_unstable_by_key(|(field, _)| *field);
    let mut bytes = Vec::new();
    for (field, value) in fields {
        match value {
            Value::Number(value) => append_signed(&mut bytes, field, value),
            Value::Text(value) => append_bytes(&mut bytes, field, value.as_bytes()),
        }
    }
    bytes
}

fn boot_id() -> &'static str {
    static BOOT_ID: OnceLock<String> = OnceLock::new();
    BOOT_ID.get_or_init(|| {
        let raw = match std::env::consts::OS {
            "macos" => std::process::Command::new("/usr/sbin/sysctl")
                .args(["-n", "kern.bootsessionuuid"])
                .output()
                .ok()
                .filter(|result| result.status.success())
                .and_then(|result| String::from_utf8(result.stdout).ok()),
            "linux" | "android" => std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok(),
            _ => None,
        };
        raw.map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| {
                value.len() == 36
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
            })
            .unwrap_or_default()
    })
}

pub(super) fn rom_version(device: &DeviceProfile) -> &str {
    let version = device.get("rom_version");
    if version.is_empty() {
        return fingerprint_for_cdid(device.get("cdid")).rom_version;
    }
    version
}

pub(super) fn signing_device(device: &DeviceProfile) -> Vec<u8> {
    let bytes = device_proto(device.get("cdid"), device.get("device_id"));
    let mut output = Vec::with_capacity(bytes.len());
    let mut source = bytes.as_slice();
    while !source.is_empty() {
        let start = source;
        let Some((field, wire)) = read_field(&mut source) else {
            break;
        };
        if (22, matches!(wire, WireValue::Bytes(_))) == (field, true) {
            append_bytes(&mut output, field, device.get("os_version").as_bytes());
        } else {
            output.extend_from_slice(&start[..start.len() - source.len()]);
        }
    }
    output
}
