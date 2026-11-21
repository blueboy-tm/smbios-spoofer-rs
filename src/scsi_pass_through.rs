// WDK ntddscsi.h layout. DataBufferOffset is ULONG_PTR, not ULONG.
// Consumers use offsets into byte slices rather than casting potentially unaligned buffers.
#[repr(C)]
pub(crate) struct ScsiPassThrough {
    pub(crate) length: u16,
    pub(crate) scsi_status: u8,
    pub(crate) path_id: u8,
    pub(crate) target_id: u8,
    pub(crate) lun: u8,
    pub(crate) cdb_length: u8,
    pub(crate) sense_info_length: u8,
    pub(crate) data_in: u8,
    pub(crate) transfer_length: u32,
    pub(crate) timeout: u32,
    pub(crate) data_buffer_offset: usize,
    pub(crate) sense_info_offset: u32,
    pub(crate) cdb: [u8; 16],
}
