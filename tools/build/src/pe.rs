use anyhow::{Context, Result, ensure};
use regex::Regex;

const PE_SIGNATURE: u32 = 0x0000_4550;
const AMD64: u16 = 0x8664;
const PE32_PLUS: u16 = 0x20b;
const WINDOWS_GUI: u16 = 2;
const IMAGE_EXECUTABLE: u16 = 0x0002;
const IMAGE_DLL: u16 = 0x2000;
const MAX_SECTIONS: usize = 96;
const MAX_IMPORT_NAME_BYTES: usize = 1024;
const MIN_STACK_BYTES: u64 = 8 * 1024 * 1024;
const HIGH_BIT: u32 = 0x8000_0000;
const OFFSET_MASK: u32 = 0x7fff_ffff;
const SECTION_BYTES: usize = 40;
const RESOURCE_HEADER_BYTES: usize = 16;
const RESOURCE_ENTRY_BYTES: usize = 8;

fn range(bytes: &[u8], offset: usize, size: usize) -> Result<&[u8]> {
    bytes
        .get(offset..offset.checked_add(size).context("PE offset overflow")?)
        .context("Invalid Windows PE: truncated or out-of-range data")
}
fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(range(bytes, offset, 2)?.try_into()?))
}
fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(range(bytes, offset, 4)?.try_into()?))
}
fn u64_at(bytes: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(range(bytes, offset, 8)?.try_into()?))
}

struct Section {
    rva: usize,
    size: usize,
    offset: usize,
}
struct Image<'a> {
    bytes: &'a [u8],
    optional: &'a [u8],
    sections: Vec<Section>,
    imports: Vec<String>,
}

impl<'a> Image<'a> {
    fn read(bytes: &'a [u8], dll: bool) -> Result<Self> {
        ensure!(range(bytes, 0, 2)? == b"MZ", "Missing DOS signature");
        let pe = u32_at(bytes, 60)? as usize;
        ensure!(u32_at(bytes, pe)? == PE_SIGNATURE, "Missing PE signature");
        let coff = range(bytes, pe + 4, 20)?;
        ensure!(u16_at(coff, 0)? == AMD64, "Expected AMD64 image");
        let flags = u16_at(coff, 18)?;
        ensure!(
            flags & IMAGE_EXECUTABLE != 0 && (flags & IMAGE_DLL != 0) == dll,
            "Wrong PE image kind"
        );
        let count = usize::from(u16_at(coff, 2)?);
        ensure!(
            count > 0 && count <= MAX_SECTIONS,
            "Invalid PE section count"
        );
        let optional_size = usize::from(u16_at(coff, 16)?);
        let optional = range(bytes, pe + 24, optional_size)?;
        ensure!(
            optional.len() >= 136 && u16_at(optional, 0)? == PE32_PLUS,
            "Expected PE32+"
        );
        let directories = u32_at(optional, 108)? as usize;
        ensure!(
            directories >= 3 && 112 + directories * 8 <= optional.len(),
            "Invalid PE directories"
        );
        let mut sections = Vec::new();
        for index in 0..count {
            let section = range(
                bytes,
                pe + 24 + optional_size + index * SECTION_BYTES,
                SECTION_BYTES,
            )?;
            sections.push(Section {
                rva: u32_at(section, 12)? as usize,
                size: u32_at(section, 16)? as usize,
                offset: u32_at(section, 20)? as usize,
            });
        }
        let mut image = Self {
            bytes,
            optional,
            sections,
            imports: Vec::new(),
        };
        image.imports = image.read_imports(1, 20, 12)?;
        if directories > 13 {
            image.imports.extend(image.read_imports(13, 32, 4)?);
        }
        let forbidden = Regex::new(
            r"(?i)^(?:msvcp.*|msvcr.*|vcruntime.*|ucrtbased?|api-ms-win-crt-.*|WebView2Loader)\.dll$",
        )?;
        for name in &image.imports {
            ensure!(
                !forbidden.is_match(name),
                "Forbidden Windows runtime: {name}"
            );
        }
        Ok(image)
    }

    fn map(&self, rva: usize, size: usize) -> Result<&'a [u8]> {
        let section = self
            .sections
            .iter()
            .find(|s| {
                rva >= s.rva
                    && (rva - s.rva)
                        .checked_add(size)
                        .is_some_and(|end| end <= s.size)
            })
            .context("RVA does not map to file data")?;
        range(self.bytes, section.offset + rva - section.rva, size)
    }

    fn directory(&self, index: usize) -> Result<(usize, usize)> {
        Ok((
            u32_at(self.optional, 112 + index * 8)? as usize,
            u32_at(self.optional, 116 + index * 8)? as usize,
        ))
    }

    fn read_imports(
        &self,
        index: usize,
        descriptor_size: usize,
        name_offset: usize,
    ) -> Result<Vec<String>> {
        let (rva, size) = self.directory(index)?;
        if rva == 0 && size == 0 {
            return Ok(Vec::new());
        }
        ensure!(
            rva != 0 && size >= descriptor_size,
            "Invalid import directory"
        );
        let descriptors = self.map(rva, size)?;
        let mut result = Vec::new();
        for descriptor in descriptors.chunks_exact(descriptor_size) {
            if descriptor.iter().all(|b| *b == 0) {
                return Ok(result);
            }
            if descriptor_size == 32 {
                ensure!(u32_at(descriptor, 0)? == 1, "Unsupported delay import");
            }
            let name_rva = u32_at(descriptor, name_offset)? as usize;
            let mut name = Vec::new();
            for offset in 0..MAX_IMPORT_NAME_BYTES {
                let byte = self.map(name_rva + offset, 1)?[0];
                if byte == 0 {
                    break;
                }
                name.push(byte);
            }
            ensure!(
                !name.is_empty() && name.len() < MAX_IMPORT_NAME_BYTES && name.is_ascii(),
                "Invalid import name"
            );
            result.push(String::from_utf8(name)?);
        }
        anyhow::bail!("Unterminated import directory")
    }

    fn payloads(&self, root: &[u8], kind: u32) -> Result<Vec<(u32, &'a [u8])>> {
        let (_, entry) = entries(root, 0)?
            .into_iter()
            .find(|(id, _)| *id == kind)
            .context("Missing resource type")?;
        ensure!(entry & HIGH_BIT != 0, "Invalid resource type");
        let mut result = Vec::new();
        for (id, child) in entries(root, (entry & OFFSET_MASK) as usize)? {
            ensure!(
                id & HIGH_BIT == 0 && child & HIGH_BIT != 0,
                "Invalid resource name"
            );
            for (_, language) in entries(root, (child & OFFSET_MASK) as usize)? {
                ensure!(language & HIGH_BIT == 0, "Invalid resource depth");
                let data = range(root, language as usize, 16)?;
                let size = u32_at(data, 4)? as usize;
                ensure!(size > 0, "Empty resource");
                result.push((id, self.map(u32_at(data, 0)? as usize, size)?));
            }
        }
        ensure!(!result.is_empty(), "Empty resource type");
        Ok(result)
    }
}

fn entries(root: &[u8], offset: usize) -> Result<Vec<(u32, u32)>> {
    let header = range(root, offset, RESOURCE_HEADER_BYTES)?;
    let count = usize::from(u16_at(header, 12)?) + usize::from(u16_at(header, 14)?);
    let table = range(
        root,
        offset + RESOURCE_HEADER_BYTES,
        count * RESOURCE_ENTRY_BYTES,
    )?;
    table
        .as_chunks::<RESOURCE_ENTRY_BYTES>()
        .0
        .iter()
        .map(|entry| Ok((u32_at(entry, 0)?, u32_at(entry, 4)?)))
        .collect()
}

pub fn imports(bytes: &[u8], dll: bool) -> Result<Vec<String>> {
    Ok(Image::read(bytes, dll)?.imports)
}

pub fn validate(bytes: &[u8], app_id: &str) -> Result<Vec<String>> {
    let image = Image::read(bytes, false)?;
    ensure!(
        u16_at(image.optional, 68)? == WINDOWS_GUI,
        "Expected GUI subsystem"
    );
    ensure!(
        u64_at(image.optional, 72)? >= MIN_STACK_BYTES,
        "Desktop stack must retain 8 MiB"
    );
    let (rva, size) = image.directory(2)?;
    ensure!(rva != 0 && size != 0, "Missing icon and manifest resources");
    let root = image.map(rva, size)?;
    let icons = image.payloads(root, 3)?;
    for (_, bytes) in image.payloads(root, 14)? {
        let count = usize::from(u16_at(bytes, 4)?);
        ensure!(u16_at(bytes, 2)? == 1 && count > 0, "Invalid icon group");
        for entry in range(bytes, 6, count * 14)?.as_chunks::<14>().0 {
            let id = u32::from(u16_at(entry, 12)?);
            let size = u32_at(entry, 8)? as usize;
            ensure!(
                icons
                    .iter()
                    .any(|(icon_id, data)| *icon_id == id && data.len() == size),
                "Missing icon image"
            );
        }
    }
    for (_, bytes) in image.payloads(root, 24)? {
        validate_manifest(std::str::from_utf8(bytes)?, app_id)?;
    }
    Ok(image.imports)
}

fn validate_manifest(text: &str, app_id: &str) -> Result<()> {
    let xml = Regex::new(r"(?s)<!--.*?-->")?.replace_all(text, "");
    let identity = Regex::new(r#"<assemblyIdentity\b[^>]*\bname=["']([^"']+)["']"#)?;
    ensure!(
        identity
            .captures(&xml)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            == Some(app_id),
        "Wrong manifest identity"
    );
    for pattern in [
        r#"<requestedExecutionLevel\b[^>]*\blevel=["']asInvoker["']"#,
        r"<dpiAwareness\b[^>]*>\s*PerMonitorV2\s*</dpiAwareness>",
        r"<longPathAware\b[^>]*>\s*true\s*</longPathAware>",
    ] {
        ensure!(
            Regex::new(pattern)?.is_match(&xml),
            "Missing required Windows manifest contract: {pattern}"
        );
    }
    Ok(())
}
