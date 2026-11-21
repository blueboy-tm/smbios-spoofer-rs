use super::*;

const STRING_START: usize = 0xA50;
const SERIAL: &str = "0000_0000_0000_0001_00A0_7523_453E_0948.";

fn fixture(units: impl IntoIterator<Item = u16>) -> Vec<u8> {
    let mut bytes = vec![0xA5; STRING_START];
    bytes[..36].copy_from_slice(b"eui.000000000000000100A07523453E0948");
    bytes[OFFSET_FIELD..OFFSET_FIELD_END].copy_from_slice(&(STRING_START as u32).to_le_bytes());
    bytes.extend(units.into_iter().flat_map(u16::to_le_bytes));
    bytes
}

fn inspect(bytes: &[u8], expected: &str) -> Result<bool, CandidateError> {
    candidate_serial_matches(
        Attachment::Spaceport,
        DETAILED_DRIVE_IOCTL,
        0,
        bytes.len(),
        bytes.len(),
        bytes,
        expected,
    )
}

#[test]
fn compares_two_serials_without_changing_any_bytes() {
    let mut bytes = fixture(SERIAL.encode_utf16().chain([0]));
    bytes.extend([0xBB; 16]); // Unrelated namespace data.
    let before = bytes.clone();
    assert_eq!(inspect(&bytes, SERIAL), Ok(true));
    assert_eq!(inspect(&bytes, "other-disk"), Ok(false));
    assert_eq!(bytes, before);
}

#[test]
fn requires_spaceport_ioctl_and_complete_success() {
    for (attachment, ioctl, status, error) in [
        (
            Attachment::Other,
            DETAILED_DRIVE_IOCTL,
            0,
            CandidateError::UnrelatedRequest,
        ),
        (
            Attachment::Spaceport,
            0,
            0,
            CandidateError::UnrelatedRequest,
        ),
        (
            Attachment::Spaceport,
            DETAILED_DRIVE_IOCTL,
            1,
            CandidateError::UnsuccessfulCompletion,
        ),
    ] {
        assert_eq!(
            candidate_serial_matches(attachment, ioctl, status, 0, 0, &[], SERIAL),
            Err(error)
        );
    }
}

#[test]
fn bounds_completed_length_and_offset() {
    let mut bytes = fixture([0]);
    assert_eq!(
        inspect(&bytes[..OFFSET_FIELD_END - 1], SERIAL),
        Err(CandidateError::TruncatedOffsetField)
    );
    assert_eq!(
        candidate_serial_matches(
            Attachment::Spaceport,
            DETAILED_DRIVE_IOCTL,
            0,
            bytes.len(),
            bytes.len() - 1,
            &bytes,
            SERIAL
        ),
        Err(CandidateError::InvalidCompletedLength)
    );
    for offset in [0, STRING_START as u32 + 1, u32::MAX - 1] {
        bytes[OFFSET_FIELD..OFFSET_FIELD_END].copy_from_slice(&offset.to_le_bytes());
        assert_eq!(inspect(&bytes, SERIAL), Err(CandidateError::InvalidOffset));
    }
}

#[test]
fn requires_a_terminator_inside_the_completed_region_and_valid_utf16() {
    let bytes = fixture([b'A' as u16, 0]);
    assert_eq!(
        candidate_serial_matches(
            Attachment::Spaceport,
            DETAILED_DRIVE_IOCTL,
            0,
            bytes.len() - 1,
            bytes.len(),
            &bytes,
            SERIAL
        ),
        Err(CandidateError::UnterminatedOrTooLong)
    );
    assert_eq!(
        inspect(&fixture([0xD800, 0]), SERIAL),
        Err(CandidateError::MalformedUtf16)
    );
    assert_eq!(
        inspect(
            &fixture([b'A' as u16; MAX_SERIAL_UNITS + 1].into_iter().chain([0])),
            SERIAL
        ),
        Err(CandidateError::UnterminatedOrTooLong)
    );
}
