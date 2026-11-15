const START_CODE: &[u8] = &[0, 0, 0, 1];

pub(crate) fn access_unit(raw: &[u8]) -> Option<(Vec<u8>, bool, bool)> {
    let mut bytes = Vec::with_capacity(raw.len());
    let mut video = false;
    let mut sync = false;
    for unit in split(raw)? {
        if unit[0] & 0x80 != 0 || unit[1] & 7 == 0 {
            return None;
        }
        let kind = (unit[0] >> 1) & 0x3f;
        video |= kind <= 31;
        sync |= matches!(kind, 16..=23);
        bytes.extend_from_slice(START_CODE);
        bytes.extend_from_slice(unit);
    }
    Some((bytes, video, sync))
}

pub(crate) fn parameter_sets(raw: &[u8]) -> Option<Vec<u8>> {
    let mut sets = Vec::new();
    let mut found = [false; 3];
    for unit in split(raw)? {
        let kind = (unit[0] >> 1) & 0x3f;
        if matches!(kind, 32..=34) {
            found[usize::from(kind - 32)] = true;
            sets.extend_from_slice(START_CODE);
            sets.extend_from_slice(unit);
        }
    }
    found.into_iter().all(|present| present).then_some(sets)
}

pub(crate) fn split(raw: &[u8]) -> Option<Vec<&[u8]>> {
    if raw.starts_with(START_CODE) || raw.starts_with(&[0, 0, 1]) {
        let mut units = Vec::new();
        let mut start = None;
        let mut offset = 0;
        while offset < raw.len() {
            let prefix = if raw[offset..].starts_with(START_CODE) {
                4
            } else if raw[offset..].starts_with(&[0, 0, 1]) {
                3
            } else {
                offset += 1;
                continue;
            };
            if let Some(start) = start {
                units.push(&raw[start..offset]);
            }
            offset += prefix;
            start = Some(offset);
        }
        units.push(&raw[start?..]);
        return units.iter().all(|unit| unit.len() >= 2).then_some(units);
    }
    let mut units = Vec::new();
    let mut offset = 0;
    while offset < raw.len() {
        let length = u32::from_be_bytes(raw.get(offset..offset + 4)?.try_into().ok()?) as usize;
        offset += 4;
        let end = offset.checked_add(length)?;
        let unit = raw.get(offset..end)?;
        if unit.len() < 2 {
            return None;
        }
        units.push(unit);
        offset = end;
    }
    (!units.is_empty()).then_some(units)
}
