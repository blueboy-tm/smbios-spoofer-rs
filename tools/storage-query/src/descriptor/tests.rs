use super::*;

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_size(bytes: &mut [u8]) {
    write_u32(bytes, 4, bytes.len() as u32);
}

fn serial_fixture() -> Vec<u8> {
    let mut bytes = vec![0; 36];
    write_u32(&mut bytes, 0, 40);
    write_u32(&mut bytes, 24, 36);
    bytes.extend_from_slice(b"XKNVHETV1TXQNT3   \0");
    write_size(&mut bytes);
    bytes
}

fn ids_fixture() -> Vec<u8> {
    let mut bytes = vec![0; 12];
    write_u32(&mut bytes, 0, 16);
    write_u32(&mut bytes, 8, 3);
    let name = b"eui.000000000000000100A07523453E0948";
    let eui64 = [0, 0xA0, 0x75, 0x23, 0x45, 0x3E, 0x09, 0x48];
    let nguid = [
        0, 0, 0, 0, 0, 0, 0, 1, 0, 0xA0, 0x75, 0x23, 0x45, 0x3E, 0x09, 0x48,
    ];
    for (code_set, kind, value, last) in [
        (3, 8, name.as_slice(), false),
        (1, 2, eui64.as_slice(), false),
        (1, 2, nguid.as_slice(), true),
    ] {
        let mut header = [0; 16];
        write_u32(&mut header, 0, code_set);
        write_u32(&mut header, 4, kind);
        header[8..10].copy_from_slice(&(value.len() as u16).to_le_bytes());
        if !last {
            let next = (header.len() + value.len()) as u16;
            header[10..12].copy_from_slice(&next.to_le_bytes());
        }
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(value);
    }
    write_size(&mut bytes);
    bytes
}

fn duid_fixture() -> Vec<u8> {
    let mut bytes = vec![0; 20];
    write_u32(&mut bytes, 0, 1);
    write_u32(&mut bytes, 8, 20);
    bytes.extend_from_slice(&ids_fixture());
    let serial_offset = bytes.len() as u32;
    write_u32(&mut bytes, 12, serial_offset);
    bytes.extend_from_slice(&serial_fixture());
    let layout_offset = bytes.len() as u32;
    write_u32(&mut bytes, 16, layout_offset);
    let mut layout = [0; 28];
    write_u32(&mut layout, 0, 1);
    write_size(&mut layout);
    layout[12..].copy_from_slice(&[0x12; 16]);
    bytes.extend_from_slice(&layout);
    write_size(&mut bytes);
    bytes
}

#[test]
fn parses_serial_verbatim_and_accepts_absent_serial() {
    let mut bytes = serial_fixture();
    assert_eq!(
        device_serial(&bytes),
        Ok(Some(b"XKNVHETV1TXQNT3   ".as_slice()))
    );
    write_u32(&mut bytes, 24, 0);
    assert_eq!(device_serial(&bytes), Ok(None));
}

#[test]
fn rejects_bad_serial_offsets_and_strings_past_declared_size() {
    let mut bytes = serial_fixture();
    let length = bytes.len() as u32;
    for offset in [35, length, length + 1, u32::MAX] {
        write_u32(&mut bytes, 24, offset);
        assert!(device_serial(&bytes).is_err());
    }
    write_u32(&mut bytes, 24, 36);
    write_u32(&mut bytes, 4, length - 1);
    assert!(device_serial(&bytes).is_err()); // The NUL is outside Size.
    write_size(&mut bytes);
    write_u32(&mut bytes, 32, u32::MAX);
    assert!(device_serial(&bytes).is_err());
}

#[test]
fn parses_text_eui64_and_nguid_bytes() {
    let bytes = ids_fixture();
    let ids = device_ids(&bytes).unwrap();
    assert_eq!(ids.len(), 3);
    assert_eq!(
        (ids[0].code_set, ids[0].kind, ids[0].association),
        (3, 8, 0)
    );
    assert_eq!(ids[0].bytes, b"eui.000000000000000100A07523453E0948");
    assert_eq!(ids[1].bytes, [0, 0xA0, 0x75, 0x23, 0x45, 0x3E, 0x09, 0x48]);
    assert_eq!(
        ids[2].bytes,
        [0, 0, 0, 0, 0, 0, 0, 1, 0, 0xA0, 0x75, 0x23, 0x45, 0x3E, 0x09, 0x48]
    );
}

#[test]
fn rejects_invalid_identifier_lengths_counts_and_links() {
    let original = ids_fixture();
    let mut bytes = original.clone();
    write_u32(&mut bytes, 8, u32::MAX);
    assert!(device_ids(&bytes).is_err());
    bytes = original.clone();
    bytes[20..22].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(device_ids(&bytes).is_err());
    for next in [0u16, 16, u16::MAX] {
        bytes = original.clone();
        bytes[22..24].copy_from_slice(&next.to_le_bytes());
        assert!(device_ids(&bytes).is_err());
    }
    // A declared Size that truncates only the last identifier's payload.
    bytes = original;
    let short_size = (bytes.len() - 1) as u32;
    write_u32(&mut bytes, 4, short_size);
    assert!(device_ids(&bytes).is_err());
}

#[test]
fn parses_duid_sections_and_layout_signatures() {
    let mut bytes = duid_fixture();
    let result = duid(&bytes).unwrap();
    assert_eq!(result.identifiers.unwrap().len(), 3);
    assert_eq!(result.serial, Some(b"XKNVHETV1TXQNT3   ".as_slice()));
    assert_eq!(result.layout, Some(LayoutSignature::Gpt(&[0x12; 16])));
    let layout_offset = u32_at(&bytes, 16).unwrap() as usize;
    bytes[layout_offset + 8] = 1;
    write_u32(&mut bytes, layout_offset + 12, 0xFEDC_BA98);
    assert_eq!(
        duid(&bytes).unwrap().layout,
        Some(LayoutSignature::Mbr(0xFEDC_BA98))
    );
}

#[test]
fn rejects_invalid_or_overlapping_duid_sections() {
    let original = duid_fixture();
    for invalid_offset in [19, 20, original.len() as u32, u32::MAX] {
        let mut bytes = original.clone();
        write_u32(&mut bytes, 12, invalid_offset);
        assert!(duid(&bytes).is_err());
    }

    let mut bytes = original;
    let serial_offset = u32_at(&bytes, 12).unwrap() as usize;
    let layout_offset = u32_at(&bytes, 16).unwrap() as usize;
    write_u32(
        &mut bytes,
        serial_offset + 4,
        (layout_offset - serial_offset + 1) as u32,
    );
    assert!(duid(&bytes).is_err()); // The nested Size overlaps the next section.
}

#[test]
fn rejects_every_truncated_prefix_of_valid_descriptors() {
    let bytes = serial_fixture();
    for end in 0..bytes.len() {
        assert!(device_serial(&bytes[..end]).is_err(), "serial prefix {end}");
    }
    let bytes = ids_fixture();
    for end in 0..bytes.len() {
        assert!(device_ids(&bytes[..end]).is_err(), "IDs prefix {end}");
    }
    let bytes = duid_fixture();
    for end in 0..bytes.len() {
        assert!(duid(&bytes[..end]).is_err(), "DUID prefix {end}");
    }
}
