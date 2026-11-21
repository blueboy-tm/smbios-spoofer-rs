use super::*;

#[test]
fn only_encodes_the_fixed_data_in_inquiry() {
    let request = Inquiry::new(63).unwrap();
    let bytes = &request.buffer;
    assert_eq!(request.data_offset, 64);
    assert_eq!(bytes[offset_of!(ScsiPassThrough, data_in)], 1);
    assert_eq!(bytes[offset_of!(ScsiPassThrough, cdb_length)], 6);
    assert_eq!(bytes[offset_of!(ScsiPassThrough, sense_info_length)], 0);
    assert_eq!(
        &bytes[offset_of!(ScsiPassThrough, cdb)..][..16],
        &[0x12, 1, 0x80, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert!(bytes[request.data_offset..].iter().all(|&byte| byte == 0));
    assert!(Inquiry::new(2).is_err());
    assert!(Inquiry::new(u32::MAX).is_err());
    #[cfg(target_pointer_width = "64")]
    {
        assert_eq!(HEADER_SIZE, 56);
        assert_eq!(TRANSFER_LENGTH, 12);
        assert_eq!(DATA_BUFFER_OFFSET, 24);
        assert_eq!(offset_of!(ScsiPassThrough, cdb), 36);
    }
}

#[test]
fn validates_driver_returned_transfer_and_scsi_status() {
    let mut inquiry = Inquiry::new(7).unwrap();
    let offset = inquiry.data_offset;
    let raw = b"\x00\x80\x00\x02AB  ";
    inquiry.buffer[offset..offset + raw.len()].copy_from_slice(raw);
    inquiry.buffer[TRANSFER_LENGTH..][..4].copy_from_slice(&(raw.len() as u32).to_le_bytes());

    assert_eq!(inquiry.reply(offset + raw.len()), Ok(raw.as_slice()));
    assert!(inquiry.reply(offset + raw.len() - 1).is_err());
    assert!(inquiry.reply(HEADER_SIZE - 1).is_err());
    assert!(inquiry.reply(inquiry.buffer.len() + 1).is_err());

    inquiry.buffer[SCSI_STATUS] = 2;
    assert_eq!(inquiry.reply(HEADER_SIZE), Err(ReplyError::ScsiStatus(2)));
    inquiry.buffer[SCSI_STATUS] = 0;
    inquiry.buffer[DATA_BUFFER_OFFSET] ^= 1;
    assert!(inquiry.reply(offset + raw.len()).is_err());
    inquiry.buffer[DATA_BUFFER_OFFSET] ^= 1;
    inquiry.buffer[TRANSFER_LENGTH..][..4].copy_from_slice(&256u32.to_le_bytes());
    assert!(inquiry.reply(inquiry.buffer.len()).is_err());
}

#[test]
fn parses_micron_serial_and_preserves_padded_or_non_ascii_bytes() {
    let page = b"\x00\x80\x00\x280000_0000_0000_0001_00A0_7523_453E_0948.";
    assert_eq!(serial(page), Ok(&page[4..]));
    assert_eq!(
        serial(b"\x00\x80\x00\x05AB \xFF\x00padding"),
        Ok(b"AB \xFF\x00".as_slice())
    );
    assert_eq!(serial(b"\x00\x80\x00\x00"), Ok([].as_slice()));
    for end in 0..page.len() {
        assert!(serial(&page[..end]).is_err());
    }
}

#[test]
fn rejects_other_pages_and_checks_the_full_big_endian_length() {
    assert!(serial(b"\x00\x83\x00\x01A").is_err());
    assert!(serial(b"\x00\x80\x01\x01A").is_err());
}
