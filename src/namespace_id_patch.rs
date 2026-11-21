//! Narrow opt-in rewrite of the observed Micron 2400 SCSI name-string identifier.
//!
//! This is a synthetic *Windows property response* alias, NOT a mutation of the
//! actual NVMe namespace NGUID/EUI-64, its firmware, or the storage layout.
//! We reject any descriptor that differs from the single-identifier fixture.
const OLD_ID: &[u8; 36] = b"eui.000000000000000100A07523453E0948";
const IDENTIFIER_HEADER: usize = 16;
const DEVICE_ID_HEADER: usize = 12;
const DUID_HEADER: usize = 20;

fn le32(buf: &[u8], pos: usize) -> Option<usize> {
    Some(u32::from_le_bytes(buf.get(pos..pos.checked_add(4)?)?.try_into().ok()?) as usize)
}

/// Deterministic, 128-bit *synthetic* alias from the already persisted v9
/// serial. Non-cryptographic domain-separated hashing: not suitable as a
/// globally assigned IEEE identifier. Reproducible across restarts.
fn replacement(serial: &[u8; 20]) -> Option<[u8; 36]> {
    let mut seen = false;
    let mut padding = false;
    for &ch in serial {
        match ch {
            b' ' => padding = true,
            b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' if !padding => seen = true,
            _ => return None,
        }
    }
    if !seen { return None; }
    let mut first: u64 = 0xcbf29ce484222325;
    let mut second: u64 = 0x84222325cbf29ce4;
    for &byte in b"smbios-spoofer-rs:namespace-alias:v1".iter().chain(serial.iter()) {
        first = (first ^ byte as u64).wrapping_mul(0x100000001b3);
        second = (second ^ (byte as u64).rotate_left(1)).wrapping_mul(0x100000001b3);
    }
    let mut output = *b"eui.00000000000000000000000000000000";
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for (index, byte) in first.to_be_bytes().into_iter().chain(second.to_be_bytes()).enumerate() {
        output[4 + index * 2] = HEX[(byte >> 4) as usize];
        output[5 + index * 2] = HEX[(byte & 15) as usize];
    }
    Some(output)
}

/// A STORAGE_DEVICE_ID_DESCRIPTOR with exactly one UTF-8 device-associated
/// SCSI name-string identifier, matching the observed 36-byte old value.
/// Validates the full response before mutating only the 36 identifier bytes.
pub fn rewrite_device_id(response: &mut [u8], serial: &[u8; 20]) -> bool {
    let Some(new_id) = replacement(serial) else { return false };
    let Some(size) = le32(response, 4) else { return false };
    if size < DEVICE_ID_HEADER + IDENTIFIER_HEADER + OLD_ID.len() || size > response.len() {
        return false;
    }
    if le32(response, 8) != Some(1) { return false; }
    let header = &response[DEVICE_ID_HEADER..DEVICE_ID_HEADER + IDENTIFIER_HEADER];
    if le32(header, 0) != Some(3) // UTF-8
        || le32(header, 4) != Some(8) // SCSI name string
        || le32(header, 12) != Some(0) // device association
        || u16::from_le_bytes([header[8], header[9]]) as usize != OLD_ID.len()
        || u16::from_le_bytes([header[10], header[11]]) != 0 // one identifier, no chain
    {
        return false;
    }
    let start = DEVICE_ID_HEADER + IDENTIFIER_HEADER;
    let Some(id) = response.get_mut(start..start + OLD_ID.len()) else {
        return false;
    };
    if id != OLD_ID { return false; }
    id.copy_from_slice(&new_id);
    true
}

/// Rewrites only the ID_DESCRIPTOR section of a DUID. Does not change its
/// embedded STORAGE_DEVICE_DESCRIPTOR, layout signature, offsets, or sizes.
pub fn rewrite_duid_id(response: &mut [u8], serial: &[u8; 20]) -> bool {
    let Some(size) = le32(response, 4) else { return false };
    if le32(response, 0) != Some(1) || size < DUID_HEADER || size > response.len() {
        return false;
    }
    let Some(id_offset) = le32(response, 8) else { return false };
    let Some(device_offset) = le32(response, 12) else { return false };
    let Some(layout_offset) = le32(response, 16) else { return false };
    if id_offset < DUID_HEADER || id_offset >= size { return false; }
    let mut id_end = size;
    for other in [device_offset, layout_offset] {
        if other == 0 { continue; }
        if other < DUID_HEADER || other >= size || other == id_offset { return false; }
        if other > id_offset { id_end = id_end.min(other); }
    }
    if id_end <= id_offset { return false; }
    rewrite_device_id(&mut response[id_offset..id_end], serial)
}

#[cfg(test)]
mod tests {
    use super::*;
    const SERIAL: &[u8; 20] = b"D1F9V96CQUN4DJ7     ";

    fn id_descriptor() -> Vec<u8> {
        let mut out = vec![0u8; 64];
        out[0..4].copy_from_slice(&12u32.to_le_bytes());
        out[4..8].copy_from_slice(&64u32.to_le_bytes());
        out[8..12].copy_from_slice(&1u32.to_le_bytes());
        out[12..16].copy_from_slice(&3u32.to_le_bytes());
        out[16..20].copy_from_slice(&8u32.to_le_bytes());
        out[20..22].copy_from_slice(&36u16.to_le_bytes());
        out[28..64].copy_from_slice(OLD_ID);
        out
    }

    fn duid() -> Vec<u8> {
        let mut out = vec![0xCCu8; 20 + 64 + 52];
        out[0..4].copy_from_slice(&1u32.to_le_bytes());
        let size = out.len() as u32;
        out[4..8].copy_from_slice(&size.to_le_bytes());
        out[8..12].copy_from_slice(&20u32.to_le_bytes());
        out[12..16].copy_from_slice(&84u32.to_le_bytes());
        out[16..20].copy_from_slice(&0u32.to_le_bytes());
        out[20..84].copy_from_slice(&id_descriptor());
        out
    }

    #[test]
    fn direct_id_rewrite_preserves_metadata_and_is_deterministic() {
        let mut out = id_descriptor();
        let before = out.clone();
        assert!(rewrite_device_id(&mut out, SERIAL));
        assert_eq!(&out[..28], &before[..28]);
        assert_ne!(&out[28..64], OLD_ID);
        assert_eq!(&out[28..32], b"eui.");
        assert!(out[32..64].iter().all(u8::is_ascii_hexdigit));
        assert!(!rewrite_device_id(&mut out, SERIAL));
        let mut other = id_descriptor();
        assert!(rewrite_device_id(&mut other, SERIAL));
        assert_eq!(out, other);
    }

    #[test]
    fn duid_rewrites_only_embedded_id() {
        let mut out = duid();
        let before = out.clone();
        assert!(rewrite_duid_id(&mut out, SERIAL));
        assert_eq!(&out[..48], &before[..48]);
        assert_ne!(&out[48..84], &before[48..84]);
        assert_eq!(&out[84..], &before[84..]);
        let mut standalone = id_descriptor();
        assert!(rewrite_device_id(&mut standalone, SERIAL));
        assert_eq!(&out[48..84], &standalone[28..64]);
    }

    #[test]
    fn rejects_malformed_or_unexpected_without_writes() {
        let mut cases = Vec::new();
        let mut wrong = id_descriptor(); wrong[28] = b'X'; cases.push(wrong);
        let mut count = id_descriptor(); count[8] = 2; cases.push(count);
        let mut truncated = id_descriptor(); truncated.truncate(62); cases.push(truncated);
        let mut type_wrong = id_descriptor(); type_wrong[16] = 2; cases.push(type_wrong);
        let mut chained = id_descriptor(); chained[22] = 64; cases.push(chained);
        let mut scope = id_descriptor(); scope[24] = 1; cases.push(scope);
        for mut case in cases {
            let original = case.clone();
            assert!(!rewrite_device_id(&mut case, SERIAL));
            assert_eq!(case, original);
        }
        let mut d = duid(); d[12..16].copy_from_slice(&20u32.to_le_bytes());
        let original = d.clone();
        assert!(!rewrite_duid_id(&mut d, SERIAL));
        assert_eq!(d, original);
    }

    #[test]
    fn invalid_profile_is_noop() {
        let mut out = id_descriptor();
        let before = out.clone();
        assert!(!rewrite_device_id(&mut out, &[b' ';20]));
        assert_eq!(out, before);
        let mut serial = *SERIAL;
        serial[17] = b'X';
        assert!(!rewrite_device_id(&mut out, &serial));
    }
}
