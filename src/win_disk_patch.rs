//! Windows Kernel Storage IOCTL buffer patchers (`no_std`).
//!
//! Shared between the UEFI/host test suite and the `smbios-disk-filter.sys`
//! Windows kernel filter driver (`driver/`).
//!
//! Handles all 5 Windows disk serial query vectors:
//! 1. `IOCTL_STORAGE_QUERY_PROPERTY` (`0x002D1400`):
//!    - `StorageDeviceProperty (0)` -> `STORAGE_DEVICE_DESCRIPTOR`
//!    - `StorageDeviceUniqueIdProperty (3)` -> embedded serial, optionally exact-matched ID
//!    - `StorageDeviceIdProperty (2)` -> optional exact-matched SCSI name-string alias
//!    - `StorageAdapterProtocolSpecificProperty (49)` / `StorageDeviceProtocolSpecificProperty (50)`
//!      -> `STORAGE_PROTOCOL_DATA_DESCRIPTOR` (NVMe 4096-byte Identify Controller & ATA 512-byte Identify)
//!    - `StorageAdapterSerialNumberProperty (56)` -> `STORAGE_ADAPTER_SERIAL_NUMBER` (UTF-16LE)
//! 2. `IOCTL_STORAGE_PROTOCOL_COMMAND` (`0x002DD3C0` / `0x002D13C0`)
//! 3. `SMART_RCV_DRIVE_DATA` (`0x0007C088`) -> `SENDCMDOUTPARAMS` (ATA word-swapped serial)
//! 4. `IOCTL_ATA_PASS_THROUGH` (`0x0004D02C`) -> `ATA_PASS_THROUGH_EX`
//! 5. `IOCTL_SCSI_PASS_THROUGH` (`0x0004D004`) / SCSI `INQUIRY` VPD Page `0x80`

use core::mem::{offset_of, size_of};

#[path = "scsi_pass_through.rs"]
mod scsi_pass_through;
use scsi_pass_through::ScsiPassThrough;
#[path = "namespace_id_patch.rs"]
mod namespace_id_patch;

const SCSI_HEADER_SIZE: usize = size_of::<ScsiPassThrough>();
const SCSI_TRANSFER_LENGTH: usize = offset_of!(ScsiPassThrough, transfer_length);
const SCSI_DATA_OFFSET: usize = offset_of!(ScsiPassThrough, data_buffer_offset);
const SCSI_SENSE_OFFSET: usize = offset_of!(ScsiPassThrough, sense_info_offset);
const SCSI_CDB_OFFSET: usize = offset_of!(ScsiPassThrough, cdb);

/// `IOCTL_STORAGE_QUERY_PROPERTY` (`CTL_CODE(IOCTL_STORAGE_BASE, 0x0500, METHOD_BUFFERED, FILE_ANY_ACCESS)`)
pub const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;

/// `IOCTL_STORAGE_PROTOCOL_COMMAND` (`CTL_CODE(IOCTL_STORAGE_BASE, 0x04F0, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS)`)
pub const IOCTL_STORAGE_PROTOCOL_COMMAND: u32 = 0x002D_D3C0;
/// Alternate read-only / any-access code sometimes sent for protocol commands
pub const IOCTL_STORAGE_PROTOCOL_COMMAND_ALT: u32 = 0x002D_13C0;

/// `SMART_RCV_DRIVE_DATA` (`CTL_CODE(IOCTL_DISK_BASE, 0x0022, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS)`)
pub const SMART_RCV_DRIVE_DATA: u32 = 0x0007_C088;

/// `IOCTL_ATA_PASS_THROUGH` (`CTL_CODE(IOCTL_SCSI_BASE, 0x040B, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS)`)
pub const IOCTL_ATA_PASS_THROUGH: u32 = 0x0004_D02C;

/// `IOCTL_SCSI_PASS_THROUGH` (`CTL_CODE(IOCTL_SCSI_BASE, 0x0401, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS)`)
pub const IOCTL_SCSI_PASS_THROUGH: u32 = 0x0004_D004;

// `STORAGE_PROPERTY_ID` values from `ntddstor.h`
pub const STORAGE_DEVICE_PROPERTY: u32 = 0;
pub const STORAGE_ADAPTER_PROPERTY: u32 = 1;
pub const STORAGE_DEVICE_ID_PROPERTY: u32 = 2;
pub const STORAGE_DEVICE_UNIQUE_ID_PROPERTY: u32 = 3;
pub const STORAGE_ADAPTER_PROTOCOL_SPECIFIC_PROPERTY: u32 = 49;
pub const STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY: u32 = 50;
pub const STORAGE_ADAPTER_SERIAL_NUMBER_PROPERTY: u32 = 56;
pub const STORAGE_ADAPTER_PROTOCOL_SPECIFIC_PROPERTY_EX: u32 = 67;
pub const STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY_EX: u32 = 68;

// `STORAGE_QUERY_TYPE`
pub const PROPERTY_STANDARD_QUERY: u32 = 0;

// `STORAGE_PROTOCOL_TYPE`
pub const PROTOCOL_TYPE_ATA: u32 = 2;
pub const PROTOCOL_TYPE_NVME: u32 = 3;

// `STORAGE_PROTOCOL_NVME_DATA_TYPE` / `STORAGE_PROTOCOL_ATA_DATA_TYPE`
pub const PROTOCOL_DATA_TYPE_IDENTIFY: u32 = 1;

// Completion context tags packed into the upper 32 bits of `Context`
pub const TAG_NONE: u8 = 0;
pub const TAG_STORAGE_DEVICE_DESCRIPTOR: u8 = 1;
pub const TAG_STORAGE_PROTOCOL_DESCRIPTOR: u8 = 3;
pub const TAG_STORAGE_ADAPTER_SERIAL: u8 = 4;
pub const TAG_STORAGE_PROTOCOL_COMMAND: u8 = 5;
pub const TAG_SMART_RCV_DRIVE_DATA: u8 = 6;
pub const TAG_ATA_PASS_THROUGH: u8 = 7;
pub const TAG_SCSI_PASS_THROUGH: u8 = 8;
pub const TAG_STORAGE_DEVICE_UNIQUE_ID: u8 = 9;
pub const TAG_STORAGE_DEVICE_ID: u8 = 10;

/// Inspects an incoming `IRP_MJ_DEVICE_CONTROL` request (`io_control_code` and `system_buffer`)
/// BEFORE the lower driver overwrites `SystemBuffer`, and returns the completion tag (`TAG_*`)
/// to use when the IRP completes.
pub fn classify_ioctl_request(io_control_code: u32, input_buf: &[u8]) -> u8 {
    match io_control_code {
        IOCTL_STORAGE_QUERY_PROPERTY => {
            if input_buf.len() < 8 {
                return TAG_NONE;
            }
            let property_id =
                u32::from_le_bytes([input_buf[0], input_buf[1], input_buf[2], input_buf[3]]);
            let query_type =
                u32::from_le_bytes([input_buf[4], input_buf[5], input_buf[6], input_buf[7]]);
            if query_type != PROPERTY_STANDARD_QUERY {
                return TAG_NONE;
            }
            match property_id {
                STORAGE_DEVICE_PROPERTY => TAG_STORAGE_DEVICE_DESCRIPTOR,
                STORAGE_DEVICE_ID_PROPERTY => TAG_STORAGE_DEVICE_ID, // Read-only unless opt-in namespace patch is used.
                STORAGE_DEVICE_UNIQUE_ID_PROPERTY => TAG_STORAGE_DEVICE_UNIQUE_ID,
                STORAGE_ADAPTER_PROTOCOL_SPECIFIC_PROPERTY
                | STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY
                | STORAGE_ADAPTER_PROTOCOL_SPECIFIC_PROPERTY_EX
                | STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY_EX => TAG_STORAGE_PROTOCOL_DESCRIPTOR,
                STORAGE_ADAPTER_SERIAL_NUMBER_PROPERTY => TAG_STORAGE_ADAPTER_SERIAL,
                _ => TAG_NONE,
            }
        }
        IOCTL_STORAGE_PROTOCOL_COMMAND | IOCTL_STORAGE_PROTOCOL_COMMAND_ALT => {
            TAG_STORAGE_PROTOCOL_COMMAND
        }
        SMART_RCV_DRIVE_DATA => TAG_SMART_RCV_DRIVE_DATA,
        IOCTL_ATA_PASS_THROUGH => TAG_ATA_PASS_THROUGH,
        IOCTL_SCSI_PASS_THROUGH if scsi_vpd80_transfer(input_buf).is_some() => {
            TAG_SCSI_PASS_THROUGH
        }
        _ => TAG_NONE,
    }
}

/// Dispatches the appropriate in-place buffer patcher based on `tag`.
pub fn patch_ioctl_output_by_tag(tag: u8, buf: &mut [u8], new_serial: &[u8; 20]) -> bool {
    match tag {
        TAG_STORAGE_DEVICE_DESCRIPTOR => patch_storage_device_descriptor(buf, new_serial),
        TAG_STORAGE_DEVICE_UNIQUE_ID => patch_storage_device_unique_id(buf, new_serial),
        TAG_STORAGE_PROTOCOL_DESCRIPTOR => patch_storage_protocol_descriptor(buf, new_serial),
        TAG_STORAGE_ADAPTER_SERIAL => patch_storage_adapter_serial_number(buf, new_serial),
        TAG_STORAGE_PROTOCOL_COMMAND => patch_storage_protocol_command(buf, new_serial),
        TAG_SMART_RCV_DRIVE_DATA => patch_smart_rcv_drive_data(buf, new_serial),
        TAG_ATA_PASS_THROUGH => patch_ata_pass_through(buf, new_serial),
        TAG_SCSI_PASS_THROUGH => patch_scsi_pass_through(buf, new_serial),
        _ => false,
    }
}

/// Opt-in second pass for the exact observed Micron SCSI name string.
/// The serial-only path remains the default when the driver feature is off.
pub fn patch_ioctl_output_with_namespace_id(
    tag: u8,
    buf: &mut [u8],
    new_serial: &[u8; 20],
) -> bool {
    match tag {
        TAG_STORAGE_DEVICE_ID => namespace_id_patch::rewrite_device_id(buf, new_serial),
        TAG_STORAGE_DEVICE_UNIQUE_ID => {
            // Keep the already working embedded serial rewrite and additionally
            // update the independent identifier descriptor, if it exactly matches.
            let serial = patch_storage_device_unique_id(buf, new_serial);
            let id = namespace_id_patch::rewrite_duid_id(buf, new_serial);
            serial || id
        }
        _ => patch_ioctl_output_by_tag(tag, buf, new_serial),
    }
}

/// Patches an ASCII serial number slice in-place while preserving:
/// - `stornvme.sys` namespace suffixes such as `"   _00000001."` (`20-byte SN` + `_` + `8-hex NSID` + `.`)
/// - Leading/trailing ASCII space padding (`0x20`)
/// - Hex-encoded serials (when `disk.sys` hex-encodes a 20-byte serial into 40 hex chars)
pub fn patch_ascii_serial_slice(serial_slice: &mut [u8], new_serial: &[u8; 20]) -> bool {
    // Some responses expose a namespace designator where a unit serial is expected.
    let unpadded = serial_slice.trim_ascii_start();
    if unpadded.is_empty()
        || unpadded
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"eui."))
    {
        return false;
    }

    // Case 1: Check for `stornvme.sys` VPD 0x80 synthesized format:
    // `[20-byte Controller SN]_[8-char hex NSID].` (30 bytes total, e.g. `CDA7N706410107T15   _00000001.`)
    if let Some(underscore_pos) = find_stornvme_nsid_suffix(serial_slice) {
        let prefix = &mut serial_slice[..underscore_pos];
        overwrite_preserving_spaces(prefix, new_serial);
        return true;
    }

    // Case 2: 40-char hex-encoded serial (used by `disk.sys` for some SATA/SCSI drives)
    if serial_slice.len() == 40 && serial_slice.iter().all(|&b| b.is_ascii_hexdigit()) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for (i, &b) in new_serial.iter().enumerate() {
            serial_slice[i * 2] = HEX[(b >> 4) as usize];
            serial_slice[i * 2 + 1] = HEX[(b & 0x0F) as usize];
        }
        return true;
    }

    // Case 3: Standard ASCII serial string
    overwrite_preserving_spaces(serial_slice, new_serial);
    true
}

/// Detects the `_00000001.` suffix synthesized by `stornvme.sys` (`_<8 hex digits>.`)
/// and returns the index of the `_` separator.
fn find_stornvme_nsid_suffix(s: &[u8]) -> Option<usize> {
    // Look for `_XXXXXXXX.` at the end of the string (10 bytes: `_` + 8 hex digits + `.`)
    if s.len() < 22 {
        return None;
    }
    let suffix_start = s.len() - 10;
    let suffix = &s[suffix_start..];
    if suffix[0] == b'_' && suffix[9] == b'.' && suffix[1..9].iter().all(|&c| c.is_ascii_hexdigit())
    {
        return Some(suffix_start);
    }
    None
}

/// Overwrites `target` with characters from `new_serial` while preserving leading and trailing
/// spaces (`0x20`) in `target`.
fn overwrite_preserving_spaces(target: &mut [u8], new_serial: &[u8; 20]) {
    let mut src_idx = 0usize;
    for dst in target.iter_mut() {
        if *dst == b' ' {
            continue;
        }
        // Advance `src_idx` to the next non-space printable character in `new_serial`
        while src_idx < new_serial.len() && new_serial[src_idx] == b' ' {
            src_idx += 1;
        }
        if src_idx < new_serial.len() {
            *dst = new_serial[src_idx];
            src_idx += 1;
        } else {
            // If the Windows-reported serial field is longer than the
            // controller serial, use ASCII space padding. Repeating the first
            // byte used to create garbage suffixes like "CCCCCCCC" and must
            // never happen. The stornvme NSID suffix is handled separately.
            *dst = b' ';
        }
    }
}

/// 1A. Patches `STORAGE_DEVICE_DESCRIPTOR` (`IOCTL_STORAGE_QUERY_PROPERTY`, `PropertyId = 0`).
///
/// Layout (`ntddstor.h`):
/// - `0..4`: `Version: u32`
/// - `4..8`: `Size: u32`
/// - `24..28`: `SerialNumberOffset: u32` (byte offset to null-terminated ASCII string)
pub fn patch_storage_device_descriptor(buf: &mut [u8], new_serial: &[u8; 20]) -> bool {
    if buf.len() < 36 {
        return false;
    }
    let serial_offset = u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]) as usize;
    if serial_offset < 36 || serial_offset >= buf.len() {
        return false;
    }

    let serial = &mut buf[serial_offset..];
    let Some(end) = serial.iter().position(|&byte| byte == 0) else {
        return false;
    };
    patch_ascii_serial_slice(&mut serial[..end], new_serial)
}

/// Patches only the serial in the DUID's embedded `STORAGE_DEVICE_DESCRIPTOR`.
fn patch_storage_device_unique_id(buf: &mut [u8], new_serial: &[u8; 20]) -> bool {
    const HEADER_SIZE: usize = 20;
    if buf.len() < HEADER_SIZE {
        return false;
    }
    let size = u32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
    let id_offset = u32::from_le_bytes(buf[8..12].try_into().unwrap()) as usize;
    let device_offset = u32::from_le_bytes(buf[12..16].try_into().unwrap()) as usize;
    let layout_offset = u32::from_le_bytes(buf[16..20].try_into().unwrap()) as usize;
    if device_offset < HEADER_SIZE || device_offset >= size || size > buf.len() {
        return false;
    }

    // Stop before any following identifier or layout signature.
    let mut end = size;
    for offset in [id_offset, layout_offset] {
        if offset == 0 {
            continue;
        }
        if offset < HEADER_SIZE || offset >= size || offset == device_offset {
            return false;
        }
        if offset > device_offset {
            end = end.min(offset);
        }
    }
    let device = &mut buf[device_offset..end];
    let Some(size_bytes) = device.get(4..8) else {
        return false;
    };
    let device_size = u32::from_le_bytes(size_bytes.try_into().unwrap()) as usize;
    let Some(device) = device.get_mut(..device_size) else {
        return false;
    };
    patch_storage_device_descriptor(device, new_serial)
}

/// 1C. Patches `STORAGE_PROTOCOL_DATA_DESCRIPTOR` (`IOCTL_STORAGE_QUERY_PROPERTY`, `PropertyId = 49 / 50`).
///
/// Layout (`ntddstor.h`):
/// - `0..4`: `Version: u32`
/// - `4..8`: `Size: u32`
/// - `8..48`: `STORAGE_PROTOCOL_SPECIFIC_DATA`:
///   - `8..12`: `ProtocolType: u32` (`2 = ATA`, `3 = NVMe`)
///   - `12..16`: `DataType: u32` (`1 = Identify`)
///   - `16..20`: `ProtocolDataRequestValue: u32` (`1 = NVME_IDENTIFY_CNS_CONTROLLER`)
///   - `24..28`: `ProtocolDataOffset: u32` (offset from start of `STORAGE_PROTOCOL_SPECIFIC_DATA`, i.e. byte 8!)
///   - `28..32`: `ProtocolDataLength: u32`
pub fn patch_storage_protocol_descriptor(buf: &mut [u8], new_serial: &[u8; 20]) -> bool {
    if buf.len() < 48 {
        return false;
    }
    let protocol_type = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    let data_type = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
    let request_val = u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]);
    let data_offset = u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]) as usize;
    let data_len = u32::from_le_bytes([buf[28], buf[29], buf[30], buf[31]]) as usize;

    if data_type != PROTOCOL_DATA_TYPE_IDENTIFY {
        return false;
    }

    // `ProtocolDataOffset` is measured from the start of `STORAGE_PROTOCOL_SPECIFIC_DATA` (offset 8)
    let abs_offset = 8usize.saturating_add(data_offset);
    if data_offset < 40 || abs_offset >= buf.len() {
        return false;
    }

    if protocol_type == PROTOCOL_TYPE_NVME {
        // Only Identify Controller (CNS = 1) defines a serial at bytes 4..24.
        if request_val == 1 && abs_offset + 64 <= buf.len() && data_len >= 64 {
            buf[abs_offset + 4..abs_offset + 24].copy_from_slice(new_serial);
            return true;
        }
    } else if protocol_type == PROTOCOL_TYPE_ATA && abs_offset + 40 <= buf.len() && data_len >= 40 {
        write_ata_word_swapped(&mut buf[abs_offset + 20..abs_offset + 40], new_serial);
        return true;
    }

    false
}

/// 1D. Patches `STORAGE_ADAPTER_SERIAL_NUMBER` (`IOCTL_STORAGE_QUERY_PROPERTY`, `PropertyId = 56`).
///
/// Layout (`ntddstor.h`):
/// - `0..4`: `Version: u32`
/// - `4..8`: `Size: u32`
/// - `8..264`: `WCHAR SerialNumber[128]` (UTF-16LE null-terminated string)
pub fn patch_storage_adapter_serial_number(buf: &mut [u8], new_serial: &[u8; 20]) -> bool {
    if buf.len() < 48 {
        return false;
    }
    let wchar_bytes = &mut buf[8..];
    let max_chars = core::cmp::min(wchar_bytes.len() / 2, 127);
    let mut char_len = 0usize;
    while char_len < max_chars {
        let lo = wchar_bytes[char_len * 2];
        let hi = wchar_bytes[char_len * 2 + 1];
        if lo == 0 && hi == 0 {
            break;
        }
        char_len += 1;
    }
    if char_len == 0 {
        return false;
    }

    let write_len = core::cmp::min(char_len, new_serial.len());
    for i in 0..write_len {
        wchar_bytes[i * 2] = new_serial[i];
        wchar_bytes[i * 2 + 1] = 0;
    }
    true
}

/// 2. Patches `STORAGE_PROTOCOL_COMMAND` (`IOCTL_STORAGE_PROTOCOL_COMMAND`).
///
/// Layout (`ntddstor.h`):
/// - `8..12`: `ProtocolType: u32` (`3 = ProtocolTypeNvme`)
/// - `24..28`: `CommandLength: u32`
/// - `36..40`: `DataFromDeviceTransferLength: u32`
/// - `52..56`: `DataFromDeviceBufferOffset: u32`
/// - `80..`: `Command` (64 bytes for NVMe, with CNS at byte 40)
pub fn patch_storage_protocol_command(buf: &mut [u8], new_serial: &[u8; 20]) -> bool {
    const COMMAND_OFFSET: usize = 80;
    const NVME_COMMAND_SIZE: usize = 64;
    const NVME_IDENTIFY_OPCODE: u8 = 0x06;
    const NVME_IDENTIFY_CONTROLLER_CNS: u8 = 1;
    if buf.len() < COMMAND_OFFSET + NVME_COMMAND_SIZE {
        return false;
    }
    let protocol_type = u32::from_le_bytes(buf[8..12].try_into().unwrap());
    let command_len = u32::from_le_bytes(buf[24..28].try_into().unwrap()) as usize;
    let from_len = u32::from_le_bytes(buf[36..40].try_into().unwrap()) as usize;
    let from_offset = u32::from_le_bytes(buf[52..56].try_into().unwrap()) as usize;

    if protocol_type != PROTOCOL_TYPE_NVME
        || command_len != NVME_COMMAND_SIZE
        || buf[COMMAND_OFFSET] != NVME_IDENTIFY_OPCODE
        || buf[COMMAND_OFFSET + 40] != NVME_IDENTIFY_CONTROLLER_CNS
        || from_len < 64
        || from_offset < COMMAND_OFFSET + NVME_COMMAND_SIZE
    {
        return false;
    }
    let Some(data) = buf.get_mut(from_offset..) else {
        return false;
    };
    if data.len() < 64 {
        return false;
    }

    data[4..24].copy_from_slice(new_serial);
    true
}

/// 3. Patches `SENDCMDOUTPARAMS` (`SMART_RCV_DRIVE_DATA`).
///
/// Layout (`ntdddisk.h`, `#pragma pack(1)`):
/// - `0..4`: `cBufferSize: u32`
/// - `4..16`: `DriverStatus: DRIVERSTATUS` (12 bytes)
/// - `16..528`: `bBuffer: [u8; 512]` (ATA `IDENTIFY DEVICE` sector; serial at `bBuffer[20..40]`)
pub fn patch_smart_rcv_drive_data(buf: &mut [u8], new_serial: &[u8; 20]) -> bool {
    // Offset 16 + 40 = 56 bytes minimum
    if buf.len() < 56 {
        return false;
    }
    write_ata_word_swapped(&mut buf[36..56], new_serial);
    true
}

/// 4. Patches `ATA_PASS_THROUGH_EX` (`IOCTL_ATA_PASS_THROUGH`, 64-bit layout).
///
/// Layout (`ntddscsi.h` on x64):
/// - `0..2`: `Length: u16` (48 on x64)
/// - `16..20`: `DataTransferLength: u32`
/// - `24..32`: `DataBufferOffset: usize` (`u64` on x64)
pub fn patch_ata_pass_through(buf: &mut [u8], new_serial: &[u8; 20]) -> bool {
    if buf.len() < 48 {
        return false;
    }
    let transfer_len = u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]) as usize;
    let data_offset = u64::from_le_bytes([
        buf[24], buf[25], buf[26], buf[27], buf[28], buf[29], buf[30], buf[31],
    ]) as usize;

    if transfer_len >= 512 && data_offset >= 40 && data_offset + 40 <= buf.len() {
        let sector = &mut buf[data_offset..];
        if is_printable_ascii(&sector[20..40]) {
            write_ata_word_swapped(&mut sector[20..40], new_serial);
            return true;
        }
    }
    false
}

/// Validate the request and return its data offset and transfer length. For a
/// data-in command the dispatch buffer may contain only the fixed header.
fn scsi_vpd80_transfer(buf: &[u8]) -> Option<(usize, usize)> {
    let header = buf.first_chunk::<SCSI_HEADER_SIZE>()?;
    if u16::from_le_bytes([header[0], header[1]]) as usize != SCSI_HEADER_SIZE
        || header[offset_of!(ScsiPassThrough, cdb_length)] != 6
        || header[offset_of!(ScsiPassThrough, data_in)] != 1
    // SCSI_IOCTL_DATA_IN
    {
        return None;
    }

    let &[0x12, 0x01, 0x80, 0, allocation, 0] = header[SCSI_CDB_OFFSET..].first_chunk::<6>()?
    else {
        return None;
    };
    let allocation = usize::from(allocation);
    let transfer_length =
        u32::from_le_bytes(*header[SCSI_TRANSFER_LENGTH..].first_chunk::<4>()?) as usize;
    let data_offset = usize::from_le_bytes(*header[SCSI_DATA_OFFSET..].first_chunk()?);
    if data_offset < SCSI_HEADER_SIZE || !(4..=allocation).contains(&transfer_length) {
        return None;
    }
    let sense_length = usize::from(header[offset_of!(ScsiPassThrough, sense_info_length)]);
    if sense_length != 0 {
        let sense_offset =
            u32::from_le_bytes(*header[SCSI_SENSE_OFFSET..].first_chunk::<4>()?) as usize;
        if sense_offset < SCSI_HEADER_SIZE || sense_offset.checked_add(sense_length)? > data_offset
        {
            return None;
        }
    }
    Some((data_offset, transfer_length))
}

/// Patches only the Unit Serial Number page of a successful buffered INQUIRY.
pub fn patch_scsi_pass_through(buf: &mut [u8], new_serial: &[u8; 20]) -> bool {
    let Some((data_offset, transfer_length)) = scsi_vpd80_transfer(buf) else {
        return false;
    };
    if buf[offset_of!(ScsiPassThrough, scsi_status)] != 0 {
        return false;
    }
    let Some(data) = buf
        .get_mut(data_offset..)
        .and_then(|data| data.get_mut(..transfer_length))
    else {
        return false;
    };
    let Some(&[_, 0x80, high, low]) = data.first_chunk() else {
        return false;
    };
    let page_length = usize::from(u16::from_be_bytes([high, low]));
    let Some(serial) = data.get_mut(4..4 + page_length) else {
        return false;
    };
    if !serial.iter().all(|b| (b' '..=b'~').contains(b)) {
        return false;
    }
    patch_ascii_serial_slice(serial, new_serial)
}

#[inline]
fn write_ata_word_swapped(dst_20: &mut [u8], new_serial: &[u8; 20]) {
    for i in (0..20).step_by(2) {
        dst_20[i] = new_serial[i + 1];
        dst_20[i + 1] = new_serial[i];
    }
}

#[inline]
fn is_printable_ascii(bytes: &[u8]) -> bool {
    let mut non_space = 0usize;
    for &b in bytes {
        if !(0x20..=0x7E).contains(&b) {
            return false;
        }
        if b != b' ' {
            non_space += 1;
        }
    }
    non_space >= 4
}

#[cfg(test)]
mod tests {
    use super::*;

    const MICRON_VPD80: &[u8; 44] = b"\x00\x80\x00\x280000_0000_0000_0001_00A0_7523_453E_0948.";

    fn micron_vpd80_completion() -> [u8; 100] {
        // Independent x64 WDK fixture: 56-byte header, then the 44-byte reply.
        let mut buf = [0; 100];
        buf[..2].copy_from_slice(&56u16.to_le_bytes());
        buf[6] = 6; // CdbLength
        buf[8] = 1; // SCSI_IOCTL_DATA_IN
        buf[12..16].copy_from_slice(&44u32.to_le_bytes());
        buf[16..20].copy_from_slice(&10u32.to_le_bytes());
        buf[24..32].copy_from_slice(&56u64.to_le_bytes());
        buf[36..42].copy_from_slice(&[0x12, 1, 0x80, 0, 255, 0]);
        buf[56..].copy_from_slice(MICRON_VPD80);
        buf
    }

    #[test]
    fn test_scsi_pass_through_updates_exact_micron_vpd80_serial() {
        let mut buf = micron_vpd80_completion();
        let mut request: [u8; 56] = buf[..56].try_into().unwrap();
        request[12..16].copy_from_slice(&255u32.to_le_bytes()); // Original allocation before underrun.
        let tag = classify_ioctl_request(IOCTL_SCSI_PASS_THROUGH, &request);
        assert_eq!(tag, TAG_SCSI_PASS_THROUGH);
        let mut expected = buf;
        expected[60..].fill(b' ');
        expected[60..75].copy_from_slice(b"XKNVHETV1TXQNT3");
        let serial = b"XKNVHETV1TXQNT3     ";

        assert!(patch_ioctl_output_by_tag(tag, &mut buf, serial));
        assert_eq!(buf, expected);
    }

    #[test]
    fn test_scsi_pass_through_rejects_other_requests_at_dispatch_and_completion() {
        for (offset, value) in [
            (0, 44),    // 32-bit SCSI_PASS_THROUGH header length
            (6, 10),    // CdbLength
            (8, 0),     // Data-out
            (8, 2),     // Unspecified direction
            (36, 0x28), // READ(10) opcode
            (37, 0),    // Standard INQUIRY
            (37, 3),    // EVPD and a reserved bit
            (38, 0x83), // Device Identification
            (39, 1),    // Unsupported high allocation byte
            (40, 43),   // Allocation length smaller than the transfer
            (41, 1),    // Nonstandard control byte
        ] {
            let mut buf = micron_vpd80_completion();
            buf[offset] = value;
            let original = buf;
            assert_eq!(
                classify_ioctl_request(IOCTL_SCSI_PASS_THROUGH, &buf[..56]),
                TAG_NONE
            );
            assert!(!patch_ioctl_output_by_tag(
                TAG_SCSI_PASS_THROUGH,
                &mut buf,
                b"XKNVHETV1TXQNT3     "
            ));
            assert_eq!(buf, original);
        }
    }

    #[test]
    fn test_scsi_pass_through_rejects_invalid_completion_bounds_and_status() {
        let serial = b"XKNVHETV1TXQNT3     ";
        let mut buf = micron_vpd80_completion();
        let original = buf;
        // Backing memory contains the reply, but the completion's valid slice is shorter.
        for returned in 0..buf.len() {
            assert!(!patch_scsi_pass_through(&mut buf[..returned], serial));
            assert_eq!(buf, original);
        }
        for (offset, value) in [
            (2, 2),    // CHECK CONDITION, even with successful NTSTATUS
            (7, 8),    // Sense length extending into data with the offset below
            (12, 43),  // Page exceeds the transfer length
            (12, 45),  // Transfer exceeds bytes returned
            (24, 55),  // Offset points inside the fixed header
            (24, 255), // Offset points beyond the returned buffer
            (58, 1),   // High byte of page length makes the page too large
        ] {
            let mut buf = original;
            if offset == 7 {
                buf[32..36].copy_from_slice(&56u32.to_le_bytes());
            }
            buf[offset] = value;
            let invalid = buf;
            assert!(!patch_scsi_pass_through(&mut buf, serial));
            assert_eq!(buf, invalid);
        }
        buf[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        let invalid = buf;
        assert!(!patch_scsi_pass_through(&mut buf, serial));
        assert_eq!(buf, invalid);
    }

    #[test]
    fn test_scsi_pass_through_rejects_vpd_length_exceeding_maximum_inquiry_transfer() {
        let fixture = micron_vpd80_completion();
        let mut buf = [b'A'; 56 + 255];
        buf[..56].copy_from_slice(&fixture[..56]);
        buf[12..16].copy_from_slice(&255u32.to_le_bytes());
        // A valid 255-byte request cannot hold this header plus 257 payload bytes.
        // Reading only the low byte would incorrectly accept a one-byte serial.
        buf[56..60].copy_from_slice(&[0, 0x80, 1, 1]);
        let original = buf;
        assert!(!patch_scsi_pass_through(&mut buf, b"XKNVHETV1TXQNT3     "));
        assert_eq!(buf, original);
    }

    #[test]
    fn test_scsi_pass_through_preserves_identifiers_and_non_text_payloads() {
        let serial = b"XKNVHETV1TXQNT3     ";
        let name = b"eui.000000000000000100A07523453E0948";
        let mut buf = micron_vpd80_completion();
        buf[56..64].copy_from_slice(&[0, 0x83, 0, 40, 3, 8, 0, 36]);
        buf[64..].copy_from_slice(name);
        let different_page = buf;
        assert!(!patch_scsi_pass_through(&mut buf, serial));
        assert_eq!(buf, different_page);
        buf[38] = 0x83;
        let page83 = buf;
        assert!(!patch_scsi_pass_through(&mut buf, serial));
        assert_eq!(buf, page83);

        let mut buf = micron_vpd80_completion();
        buf[59] = 36;
        buf[60..96].copy_from_slice(name);
        let eui = buf;
        assert!(!patch_scsi_pass_through(&mut buf, serial));
        assert_eq!(buf, eui);
        buf[60] = 0;
        let opaque = buf;
        assert!(!patch_scsi_pass_through(&mut buf, serial));
        assert_eq!(buf, opaque);
    }

    #[test]
    fn test_storage_device_id_preserves_reported_eui() {
        let eui = b"eui.000000000000000100A07523453E0948";
        let mut buf = [0u8; 28 + 36];
        let size = buf.len() as u32;
        buf[..4].copy_from_slice(&16u32.to_le_bytes());
        buf[4..8].copy_from_slice(&size.to_le_bytes());
        buf[8..12].copy_from_slice(&1u32.to_le_bytes());
        buf[12..16].copy_from_slice(&3u32.to_le_bytes()); // UTF-8
        buf[16..20].copy_from_slice(&8u32.to_le_bytes()); // SCSI name string
        buf[20..22].copy_from_slice(&(eui.len() as u16).to_le_bytes());
        buf[28..].copy_from_slice(eui);

        let mut request = [0; 8];
        request[..4].copy_from_slice(&STORAGE_DEVICE_ID_PROPERTY.to_le_bytes());
        let tag = classify_ioctl_request(IOCTL_STORAGE_QUERY_PROPERTY, &request);
        assert_eq!(tag, TAG_NONE);
        assert!(!patch_ioctl_output_by_tag(
            tag,
            &mut buf,
            b"YPAF58TWVPR65HW     "
        ));

        // The old 20-byte tail rewrite produced eui.000000000000YPAF58TWVPR65HW.
        assert_eq!(&buf[28..], eui);
    }

    #[test]
    fn test_storage_device_id_preserves_binary_namespace_ids() {
        let mut buf = [0u8; 12 + 16 + 8 + 16 + 16];
        let size = buf.len() as u32;
        buf[..4].copy_from_slice(&16u32.to_le_bytes());
        buf[4..8].copy_from_slice(&size.to_le_bytes());
        buf[8..12].copy_from_slice(&2u32.to_le_bytes());
        buf[12..16].copy_from_slice(&1u32.to_le_bytes()); // Binary
        buf[16..20].copy_from_slice(&2u32.to_le_bytes()); // EUI-64 type
        buf[20..22].copy_from_slice(&8u16.to_le_bytes());
        buf[22..24].copy_from_slice(&24u16.to_le_bytes()); // NextOffset
        buf[28..36].copy_from_slice(&[0x00, 0xa0, 0x75, 0x23, 0x45, 0x3e, 0x09, 0x48]);
        buf[36..40].copy_from_slice(&1u32.to_le_bytes()); // Binary
        buf[40..44].copy_from_slice(&2u32.to_le_bytes()); // EUI type with 16-byte NGUID
        buf[44..46].copy_from_slice(&16u16.to_le_bytes());
        buf[52..68].copy_from_slice(&[
            0, 0, 0, 0, 0, 0, 0, 1, 0x00, 0xa0, 0x75, 0x23, 0x45, 0x3e, 0x09, 0x48,
        ]);

        let mut request = [0; 8];
        request[..4].copy_from_slice(&STORAGE_DEVICE_ID_PROPERTY.to_le_bytes());
        let tag = classify_ioctl_request(IOCTL_STORAGE_QUERY_PROPERTY, &request);
        let original = buf;
        assert!(!patch_ioctl_output_by_tag(
            tag,
            &mut buf,
            b"YPAF58TWVPR65HW     "
        ));
        assert_eq!(buf, original);
    }

    #[test]
    fn test_storage_unique_id_patches_only_embedded_serial() {
        // Intentionally use the same hex suffix in the namespace identifier and serial.
        let serial = b"00A07523453E0948";
        let mut buf = [0u8; 20 + 28 + 36 + 36 + 16 + 1 + 8];
        let size = buf.len() as u32;
        buf[..4].copy_from_slice(&1u32.to_le_bytes());
        buf[4..8].copy_from_slice(&size.to_le_bytes());
        buf[8..12].copy_from_slice(&20u32.to_le_bytes());
        buf[12..16].copy_from_slice(&84u32.to_le_bytes());
        buf[16..20].copy_from_slice(&137u32.to_le_bytes());
        let id = &mut buf[20..84];
        id[..4].copy_from_slice(&16u32.to_le_bytes());
        id[4..8].copy_from_slice(&64u32.to_le_bytes());
        id[8..12].copy_from_slice(&1u32.to_le_bytes());
        id[12..16].copy_from_slice(&3u32.to_le_bytes());
        id[16..20].copy_from_slice(&8u32.to_le_bytes());
        id[20..22].copy_from_slice(&36u16.to_le_bytes());
        id[28..].copy_from_slice(b"eui.000000000000000100A07523453E0948");
        let device = &mut buf[84..137];
        device[..4].copy_from_slice(&36u32.to_le_bytes());
        device[4..8].copy_from_slice(&53u32.to_le_bytes());
        device[24..28].copy_from_slice(&36u32.to_le_bytes());
        device[36..52].copy_from_slice(serial);
        buf[137..].copy_from_slice(b"12345678");

        let mut request = [0; 8];
        request[..4].copy_from_slice(&STORAGE_DEVICE_UNIQUE_ID_PROPERTY.to_le_bytes());
        let tag = classify_ioctl_request(IOCTL_STORAGE_QUERY_PROPERTY, &request);
        let mut expected = buf;
        expected[120..136].copy_from_slice(b"YPAF58TWVPR65HW ");
        assert!(patch_ioctl_output_by_tag(
            tag,
            &mut buf,
            b"YPAF58TWVPR65HW     "
        ));
        assert_eq!(buf, expected);

        // An embedded descriptor claiming the layout signature's bytes must be ignored.
        buf[88..92].copy_from_slice(&54u32.to_le_bytes());
        let original = buf;
        assert!(!patch_ioctl_output_by_tag(
            tag,
            &mut buf,
            b"DIFFERENT-SERIAL    "
        ));
        assert_eq!(buf, original);
    }

    #[test]
    fn test_ascii_serial_patcher_preserves_eui_designators() {
        let mut id = *b" EUI.000000000000000100A07523453E0948";
        let original = id;
        assert!(!patch_ascii_serial_slice(&mut id, b"YPAF58TWVPR65HW     "));
        assert_eq!(id, original);
    }

    #[test]
    fn test_opaque_ioctl_outputs_are_not_scanned() {
        let request = [0xff, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            classify_ioctl_request(IOCTL_STORAGE_QUERY_PROPERTY, &request),
            TAG_NONE
        );
        assert_eq!(classify_ioctl_request(0x8000_2000, &[]), TAG_NONE);
    }

    #[test]
    fn test_long_registry_serial_pads_instead_of_repeating_first_char() {
        // Regression: a 44-character Windows-reported identifier was rewritten
        // with repeated leading serial bytes ("CCCC...") after source exhaustion.
        let mut serial = *b"0000_0000_0000_0001_00A0_7523_453E_0948.";
        let spoof = *b"CA54N299714909Q56   ";
        assert!(patch_ascii_serial_slice(&mut serial, &spoof));
        assert_eq!(&serial[..17], b"CA54N299714909Q56");
        assert!(serial[17..].iter().all(|&b| b == b' '));
    }

    #[test]
    fn test_long_registry_serial_preserves_namespace_suffix() {
        let mut serial = *b"0000_0000_0000_0001_00A0_7523_453E_0948._00000001.";
        let spoof = *b"CA54N299714909Q56   ";
        assert!(patch_ascii_serial_slice(&mut serial, &spoof));
        assert_eq!(&serial[..17], b"CA54N299714909Q56");
        assert!(serial[17..serial.len() - 10].iter().all(|&b| b == b' '));
        assert_eq!(&serial[serial.len() - 10..], b"_00000001.");
    }

    #[test]
    fn test_patch_storage_device_descriptor_stornvme_format() {
        // Build a synthetic STORAGE_DEVICE_DESCRIPTOR with `CDA7N706410107T15   _00000001.\0` at offset 40
        let mut buf = [0u8; 128];
        buf[0..4].copy_from_slice(&40u32.to_le_bytes()); // Version
        buf[4..8].copy_from_slice(&128u32.to_le_bytes()); // Size
        buf[24..28].copy_from_slice(&40u32.to_le_bytes()); // SerialNumberOffset = 40
        let orig = b"CDA7N706410107T15   _00000001.\0";
        buf[40..40 + orig.len()].copy_from_slice(orig);

        let new_serial = b"CA54N299714909Q56   ";
        let req = [0u8, 0, 0, 0, 0, 0, 0, 0]; // StorageDeviceProperty (0), PropertyStandardQuery (0)
        let tag = classify_ioctl_request(IOCTL_STORAGE_QUERY_PROPERTY, &req);
        assert_eq!(tag, TAG_STORAGE_DEVICE_DESCRIPTOR);

        assert!(patch_ioctl_output_by_tag(tag, &mut buf, new_serial));
        assert_eq!(&buf[40..70], b"CA54N299714909Q56   _00000001.");
        assert_eq!(buf[70], 0);
    }

    #[test]
    fn test_storage_device_descriptor_patches_micron_grouped_serial() {
        const SERIAL_OFFSET: usize = 40;
        const ORIGINAL_SERIAL: &[u8] = b"0000_0000_0000_0001_00A0_7523_453E_0948.\0";
        let mut buf = [0u8; SERIAL_OFFSET + ORIGINAL_SERIAL.len()];
        let size = buf.len() as u32;
        buf[..4].copy_from_slice(&40u32.to_le_bytes());
        buf[4..8].copy_from_slice(&size.to_le_bytes());
        buf[24..28].copy_from_slice(&(SERIAL_OFFSET as u32).to_le_bytes());
        buf[SERIAL_OFFSET..].copy_from_slice(ORIGINAL_SERIAL);

        // StorageDeviceProperty uses SerialNumberOffset even for a namespace-shaped value.
        let tag = classify_ioctl_request(IOCTL_STORAGE_QUERY_PROPERTY, &[0; 8]);
        assert_eq!(tag, TAG_STORAGE_DEVICE_DESCRIPTOR);
        let serial = b"XKNVHETV1TXQNT3     ";
        let mut expected = buf;
        expected[SERIAL_OFFSET..buf.len() - 1].fill(b' ');
        expected[SERIAL_OFFSET..SERIAL_OFFSET + serial.len()].copy_from_slice(serial);

        assert!(patch_ioctl_output_by_tag(tag, &mut buf, serial));
        assert_eq!(buf, expected);
    }

    #[test]
    fn test_patch_storage_protocol_descriptor_nvme_identify() {
        // 48-byte header + 4096-byte NVMe Identify Controller payload
        let mut buf = [0u8; 48 + 4096];
        buf[0..4].copy_from_slice(&48u32.to_le_bytes());
        buf[4..8].copy_from_slice(&48u32.to_le_bytes());
        buf[8..12].copy_from_slice(&PROTOCOL_TYPE_NVME.to_le_bytes());
        buf[12..16].copy_from_slice(&PROTOCOL_DATA_TYPE_IDENTIFY.to_le_bytes());
        buf[16..20].copy_from_slice(&1u32.to_le_bytes()); // CNS = 1 (Controller)
        buf[24..28].copy_from_slice(&40u32.to_le_bytes()); // Offset from byte 8 -> 8 + 40 = 48
        buf[28..32].copy_from_slice(&4096u32.to_le_bytes());

        buf[48 + 4..48 + 24].copy_from_slice(b"CDA7N706410107T15   ");
        buf[48 + 24..48 + 64].copy_from_slice(b"HFM001TD3JX013N                         ");

        let new_serial = b"CA54N299714909Q56   ";
        assert!(patch_storage_protocol_descriptor(&mut buf, new_serial));
        assert_eq!(&buf[48 + 4..48 + 24], new_serial);
        // Model number must remain untouched
        assert_eq!(
            &buf[48 + 24..48 + 64],
            b"HFM001TD3JX013N                         "
        );
    }

    #[test]
    fn test_nvme_identify_namespace_does_not_infer_a_controller_serial() {
        let mut buf = [0u8; 48 + 4096];
        buf[8..12].copy_from_slice(&PROTOCOL_TYPE_NVME.to_le_bytes());
        buf[12..16].copy_from_slice(&PROTOCOL_DATA_TYPE_IDENTIFY.to_le_bytes());
        buf[24..28].copy_from_slice(&40u32.to_le_bytes());
        buf[28..32].copy_from_slice(&4096u32.to_le_bytes());
        // A namespace identifier list may contain entirely printable payload bytes.
        buf[16..20].copy_from_slice(&3u32.to_le_bytes()); // CNS = Namespace Identification Descriptor List
        buf[48..52].copy_from_slice(&[3, 16, 0, 0]); // UUID descriptor
        buf[52..68].copy_from_slice(b"ABCDEFGHIJKLMNOP");
        buf[68..72].copy_from_slice(&[1, 8, 0, 0]); // EUI-64 descriptor
        let original = buf;
        let serial = b"YPAF58TWVPR65HW     ";
        assert!(!patch_ioctl_output_by_tag(
            TAG_STORAGE_PROTOCOL_DESCRIPTOR,
            &mut buf,
            serial
        ));
        assert_eq!(buf, original);
    }

    #[test]
    fn test_nvme_protocol_command_patches_only_identify_controller() {
        let mut buf = [0u8; 144 + 4096];
        buf[8..12].copy_from_slice(&PROTOCOL_TYPE_NVME.to_le_bytes());
        buf[24..28].copy_from_slice(&64u32.to_le_bytes());
        buf[36..40].copy_from_slice(&4096u32.to_le_bytes());
        buf[52..56].copy_from_slice(&144u32.to_le_bytes());
        buf[80] = 0x06; // Identify opcode
        buf[148..168].copy_from_slice(b"CDA7N706410107T15   ");
        let serial = b"YPAF58TWVPR65HW     ";

        let original = buf;
        assert!(!patch_storage_protocol_command(&mut buf, serial)); // CNS = 0 (Namespace)
        assert_eq!(buf, original);

        buf[120] = 1; // CNS = Identify Controller
        let mut expected = buf;
        expected[148..168].copy_from_slice(serial);
        assert!(patch_storage_protocol_command(&mut buf, serial));
        assert_eq!(buf, expected);
    }

    #[test]
    fn test_patch_smart_rcv_drive_data_word_swapped() {
        let mut buf = [0u8; 528];
        buf[0..4].copy_from_slice(&512u32.to_le_bytes());
        let new_serial = b"S4EVNF0M123456A     ";
        assert!(patch_smart_rcv_drive_data(&mut buf, new_serial));
        // Pair-swapped check: "S4" -> "4S"
        assert_eq!(buf[36], b'4');
        assert_eq!(buf[37], b'S');
        assert_eq!(buf[38], b'V');
        assert_eq!(buf[39], b'E');
    }

    #[test]
    fn test_patch_storage_adapter_serial_utf16() {
        let mut buf = [0u8; 264];
        buf[0..4].copy_from_slice(&264u32.to_le_bytes());
        buf[4..8].copy_from_slice(&264u32.to_le_bytes());
        for (i, &b) in b"CDA7N706410107T15   ".iter().enumerate() {
            buf[8 + i * 2] = b;
            buf[8 + i * 2 + 1] = 0;
        }
        let new_serial = b"CA54N299714909Q56   ";
        assert!(patch_storage_adapter_serial_number(&mut buf, new_serial));
        for (i, &b) in new_serial.iter().enumerate() {
            assert_eq!(buf[8 + i * 2], b);
            assert_eq!(buf[8 + i * 2 + 1], 0);
        }
    }
}
