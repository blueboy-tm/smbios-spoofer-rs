pub type ParseResult<T> = Result<T, &'static str>;

#[derive(Debug, PartialEq, Eq)]
pub struct Identifier<'a> {
    pub code_set: u32,
    pub kind: u32,
    pub association: u32,
    pub bytes: &'a [u8],
}

#[derive(Debug, PartialEq, Eq)]
pub enum LayoutSignature<'a> {
    Mbr(u32),
    Gpt(&'a [u8; 16]),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Duid<'a> {
    pub identifiers: Option<Vec<Identifier<'a>>>,
    pub serial: Option<&'a [u8]>,
    pub layout: Option<LayoutSignature<'a>>,
}

fn u32_at(bytes: &[u8], offset: usize) -> ParseResult<u32> {
    bytes
        .get(offset..)
        .and_then(|tail| tail.first_chunk::<4>())
        .map(|&field| u32::from_le_bytes(field))
        .ok_or("truncated 32-bit field")
}

// Every parser receives only bytes returned by the IOCTL or the bounded parent section.
fn descriptor(bytes: &[u8], minimum: usize) -> ParseResult<&[u8]> {
    let size = u32_at(bytes, 4)? as usize;
    if size < minimum {
        return Err("descriptor Size is smaller than its fixed header");
    }
    bytes
        .get(..size)
        .ok_or("descriptor Size exceeds available bytes")
}

pub fn device_serial(bytes: &[u8]) -> ParseResult<Option<&[u8]>> {
    const HEADER_SIZE: usize = 36;
    let bytes = descriptor(bytes, HEADER_SIZE)?;
    let raw_length = u32_at(bytes, 32)? as usize;
    if raw_length > bytes.len() - HEADER_SIZE {
        return Err("RawPropertiesLength exceeds the descriptor");
    }

    let offset = u32_at(bytes, 24)? as usize;
    if offset == 0 {
        return Ok(None);
    }
    if offset < HEADER_SIZE {
        return Err("SerialNumberOffset points into the descriptor header");
    }
    let tail = bytes
        .get(offset..)
        .ok_or("SerialNumberOffset exceeds the descriptor")?;
    let end = tail
        .iter()
        .position(|&byte| byte == 0)
        .ok_or("serial is missing a NUL terminator within the descriptor")?;
    Ok(Some(&tail[..end]))
}

pub fn device_ids(bytes: &[u8]) -> ParseResult<Vec<Identifier<'_>>> {
    const DESCRIPTOR_HEADER: usize = 12;
    const IDENTIFIER_HEADER: usize = 16;
    let bytes = descriptor(bytes, DESCRIPTOR_HEADER)?;
    let count = u32_at(bytes, 8)? as usize;
    let mut remaining = &bytes[DESCRIPTOR_HEADER..];
    if count > remaining.len() / IDENTIFIER_HEADER {
        return Err("NumberOfIdentifiers exceeds the descriptor");
    }

    let mut identifiers = Vec::with_capacity(count);
    for index in 0..count {
        let header = remaining
            .first_chunk::<IDENTIFIER_HEADER>()
            .ok_or("truncated STORAGE_IDENTIFIER header")?;
        let length = u16::from_le_bytes([header[8], header[9]]) as usize;
        let next = u16::from_le_bytes([header[10], header[11]]) as usize;
        let end = IDENTIFIER_HEADER + length;
        let value = remaining
            .get(IDENTIFIER_HEADER..end)
            .ok_or("IdentifierSize exceeds the descriptor")?;

        identifiers.push(Identifier {
            code_set: u32_at(header, 0)?,
            kind: u32_at(header, 4)?,
            association: u32_at(header, 12)?,
            bytes: value,
        });
        if index + 1 == count && next == 0 {
            break;
        }
        if next < end {
            return Err("NextOffset is zero or points into the current identifier");
        }
        remaining = remaining
            .get(next..)
            .ok_or("NextOffset exceeds the descriptor")?;
    }
    Ok(identifiers)
}

pub fn duid(bytes: &[u8]) -> ParseResult<Duid<'_>> {
    const HEADER_SIZE: usize = 20;
    let bytes = descriptor(bytes, HEADER_SIZE)?;
    if u32_at(bytes, 0)? != 1 {
        return Err("unsupported STORAGE_DEVICE_UNIQUE_IDENTIFIER version");
    }
    let offsets = [
        u32_at(bytes, 8)? as usize,
        u32_at(bytes, 12)? as usize,
        u32_at(bytes, 16)? as usize,
    ];
    let section = |offset| {
        if offset == 0 {
            return Ok(None);
        }
        if !(HEADER_SIZE..bytes.len()).contains(&offset) {
            return Err("DUID section offset lies outside its payload");
        }
        if offsets.iter().filter(|&&other| other == offset).count() > 1 {
            return Err("DUID section offsets overlap");
        }
        let end = offsets
            .iter()
            .copied()
            .filter(|&other| other > offset)
            .min()
            .unwrap_or(bytes.len());
        let end = end.min(bytes.len());
        Ok(Some(&bytes[offset..end]))
    };

    Ok(Duid {
        identifiers: section(offsets[0])?.map(device_ids).transpose()?,
        serial: section(offsets[1])?
            .map(device_serial)
            .transpose()?
            .flatten(),
        layout: section(offsets[2])?.map(layout_signature).transpose()?,
    })
}

fn layout_signature(bytes: &[u8]) -> ParseResult<LayoutSignature<'_>> {
    const SIGNATURE_OFFSET: usize = 12;
    let bytes = descriptor(bytes, SIGNATURE_OFFSET)?;
    match bytes[8] {
        1 => Ok(LayoutSignature::Mbr(u32_at(bytes, SIGNATURE_OFFSET)?)),
        0 => {
            let guid = bytes[SIGNATURE_OFFSET..]
                .first_chunk::<16>()
                .ok_or("truncated GPT layout signature")?;
            Ok(LayoutSignature::Gpt(guid))
        }
        _ => Err("invalid MBR flag in layout signature"),
    }
}

#[cfg(test)]
mod tests;
