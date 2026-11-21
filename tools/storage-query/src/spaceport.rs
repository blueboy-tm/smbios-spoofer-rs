//! Offline inspection of a candidate Spaceport response layout.
//! A match tests a hypothesis on one capture; it does not identify a disk,
//! establish a destination capacity, or validate a Windows wire format.

const DETAILED_DRIVE_IOCTL: u32 = 0x00E7_0408;
const OFFSET_FIELD: usize = 0xA3C;
const OFFSET_FIELD_END: usize = OFFSET_FIELD + size_of::<u32>();
// Diagnostic work limit, independent of the undocumented record format.
const MAX_SERIAL_UNITS: usize = 256;

use std::mem::size_of;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attachment {
    Spaceport,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateError {
    UnrelatedRequest,
    UnsuccessfulCompletion,
    InvalidCompletedLength,
    TruncatedOffsetField,
    InvalidOffset,
    UnterminatedOrTooLong,
    MalformedUtf16,
}

/// Test the hypothesis that 0xA3C is a little-endian u32 byte offset
/// relative to the start of the output, pointing past the field itself.
/// The expected text must come from a separate observation. The return
/// value reports only equality and exposes no payload, span, or pointer.
pub fn candidate_serial_matches(
    attachment: Attachment,
    ioctl: u32,
    status: i32,
    completed: usize,
    output_capacity: usize,
    captured_output: &[u8],
    expected: &str,
) -> Result<bool, CandidateError> {
    if attachment != Attachment::Spaceport || ioctl != DETAILED_DRIVE_IOCTL {
        return Err(CandidateError::UnrelatedRequest);
    }
    // A partial response with informational status isn't evidence of this layout.
    if status != 0 {
        return Err(CandidateError::UnsuccessfulCompletion);
    }
    if completed > output_capacity || completed > captured_output.len() {
        return Err(CandidateError::InvalidCompletedLength);
    }
    let output = &captured_output[..completed];
    let offset_bytes = output
        .get(OFFSET_FIELD..OFFSET_FIELD_END)
        .ok_or(CandidateError::TruncatedOffsetField)?;
    let offset = u32::from_le_bytes(offset_bytes.try_into().expect("four-byte field")) as usize;
    if offset < OFFSET_FIELD_END || offset & 1 != 0 || offset > completed.saturating_sub(2) {
        return Err(CandidateError::InvalidOffset);
    }

    let units = || {
        output[offset..]
            .as_chunks::<2>()
            .0
            .iter()
            .take(MAX_SERIAL_UNITS + 1)
            .map(|&pair| u16::from_le_bytes(pair))
    };
    if !units().any(|unit| unit == 0) {
        return Err(CandidateError::UnterminatedOrTooLong);
    }

    let mut expected_chars = expected.chars();
    let mut matches = true;
    for character in char::decode_utf16(units().take_while(|&unit| unit != 0)) {
        let character = character.map_err(|_| CandidateError::MalformedUtf16)?;
        matches &= expected_chars.next() == Some(character);
    }
    Ok(matches && expected_chars.next().is_none())
}

#[cfg(test)]
mod tests;
