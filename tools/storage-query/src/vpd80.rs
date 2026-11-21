use std::mem::{offset_of, size_of};

// Layout of the WDK SCSI_PASS_THROUGH header. Serialize into a zero-filled
// buffer so padding, reserved fields and unused CDB bytes are also zero.
#[repr(C)]
struct ScsiPassThrough {
    length: u16,
    scsi_status: u8,
    path_id: u8,
    target_id: u8,
    lun: u8,
    cdb_length: u8,
    sense_info_length: u8,
    data_in: u8,
    transfer_length: u32,
    timeout: u32,
    data_buffer_offset: usize,
    sense_info_offset: u32,
    cdb: [u8; 16],
}

pub const HEADER_SIZE: usize = size_of::<ScsiPassThrough>();
const TRANSFER_LENGTH: usize = offset_of!(ScsiPassThrough, transfer_length);
const DATA_BUFFER_OFFSET: usize = offset_of!(ScsiPassThrough, data_buffer_offset);
const SCSI_STATUS: usize = offset_of!(ScsiPassThrough, scsi_status);
const ALLOCATION_LENGTH: u8 = 255;
const INQUIRY_CDB: [u8; 6] = [0x12, 0x01, 0x80, 0, ALLOCATION_LENGTH, 0];

pub struct Inquiry {
    pub buffer: Vec<u8>,
    data_offset: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReplyError {
    Invalid(&'static str),
    ScsiStatus(u8),
}

impl Inquiry {
    pub fn new(alignment_mask: u32) -> Result<Self, &'static str> {
        // Support power-of-two alignment through 4096 bytes without allocating
        // an unbounded buffer from a device-provided mask.
        if alignment_mask > 4095 || !(alignment_mask + 1).is_power_of_two() {
            return Err("unsupported adapter alignment mask");
        }
        let mask = alignment_mask as usize;
        let data_offset = (HEADER_SIZE + mask) & !mask;
        let mut buffer = vec![0; data_offset + usize::from(ALLOCATION_LENGTH)];
        buffer[..2].copy_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
        buffer[offset_of!(ScsiPassThrough, cdb_length)] = INQUIRY_CDB.len() as u8;
        buffer[offset_of!(ScsiPassThrough, data_in)] = 1; // SCSI_IOCTL_DATA_IN
        buffer[TRANSFER_LENGTH..][..4].copy_from_slice(&u32::from(ALLOCATION_LENGTH).to_le_bytes());
        buffer[offset_of!(ScsiPassThrough, timeout)..][..4].copy_from_slice(&10u32.to_le_bytes());
        buffer[DATA_BUFFER_OFFSET..][..size_of::<usize>()]
            .copy_from_slice(&data_offset.to_le_bytes());
        buffer[offset_of!(ScsiPassThrough, cdb)..][..INQUIRY_CDB.len()]
            .copy_from_slice(&INQUIRY_CDB);
        Ok(Self {
            buffer,
            data_offset,
        })
    }

    pub fn reply(&self, returned: usize) -> Result<&[u8], ReplyError> {
        use ReplyError::{Invalid, ScsiStatus};

        let bytes = self
            .buffer
            .get(..returned)
            .ok_or(Invalid("invalid bytes-returned count"))?;
        let header = bytes
            .first_chunk::<HEADER_SIZE>()
            .ok_or(Invalid("truncated pass-through header"))?;
        if u16::from_le_bytes([header[0], header[1]]) as usize != HEADER_SIZE {
            return Err(Invalid("unexpected SCSI_PASS_THROUGH header length"));
        }
        let status = header[SCSI_STATUS];
        if status != 0 {
            return Err(ScsiStatus(status));
        }
        if header[DATA_BUFFER_OFFSET..][..size_of::<usize>()] != self.data_offset.to_le_bytes() {
            return Err(Invalid("unexpected data buffer offset"));
        }
        let [a, b, c, d] = *header[TRANSFER_LENGTH..].first_chunk::<4>().unwrap();
        let transferred = u32::from_le_bytes([a, b, c, d]) as usize;
        if transferred > usize::from(ALLOCATION_LENGTH) {
            return Err(Invalid("data transfer exceeded the allocation length"));
        }
        bytes
            .get(self.data_offset..self.data_offset + transferred)
            .ok_or(Invalid("data transfer exceeded the returned bytes"))
    }
}

pub fn serial(bytes: &[u8]) -> Result<&[u8], &'static str> {
    let [_, page, high, low] = *bytes.first_chunk::<4>().ok_or("truncated VPD header")?;
    if page != 0x80 {
        return Err("response is not VPD page 0x80");
    }
    let size = usize::from(u16::from_be_bytes([high, low]));
    bytes
        .get(4..4 + size)
        .ok_or("VPD page length exceeds the data transfer")
}

#[cfg(test)]
mod tests;
