use crate::descriptor::{self, Identifier, LayoutSignature, ParseResult};
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
use std::process::ExitCode;
use std::ptr;

const PATH: &str = r"\\.\PhysicalDrive0";
const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
const FILE_SHARE_READ: u32 = 1;
const FILE_SHARE_WRITE: u32 = 2;
const MAX_REPLY_BYTES: usize = 1024 * 1024;

#[link(name = "kernel32")]
extern "system" {
    fn DeviceIoControl(
        device: *mut c_void,
        control_code: u32,
        input: *const c_void,
        input_size: u32,
        output: *mut c_void,
        output_size: u32,
        returned: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
}

pub fn run() -> ExitCode {
    // Zero desired access allows metadata queries without read or write data access.
    let disk = match OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(PATH)
    {
        Ok(disk) => disk,
        Err(error) => {
            eprintln!("Could not open {PATH} for property queries: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!("Direct IOCTL results for {PATH}");
    println!(
        "These results describe this disk handle. Provider request paths require separate tracing."
    );
    let mut any_decoded = false;
    for (property, name) in [
        (0, "StorageDeviceProperty"),
        (2, "StorageDeviceIdProperty"),
        (3, "StorageDeviceUniqueIdProperty"),
    ] {
        println!("\n{name}");
        match query(&disk, property) {
            Ok(bytes) => match display(property, &bytes) {
                Ok(()) => any_decoded = true,
                Err(reason) => println!("  Invalid or unsupported descriptor: {reason}"),
            },
            Err(error) if matches!(error.raw_os_error(), Some(1 | 50)) => {
                println!("  Unsupported query: {error}");
            }
            Err(error) if matches!(error.raw_os_error(), Some(122 | 234)) => {
                println!(
                    "  Incomplete reply (output buffer limit {MAX_REPLY_BYTES} bytes): {error}"
                );
            }
            Err(error) => println!("  Query failed: {error}"),
        }
    }

    if any_decoded {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn query(disk: &File, property: u32) -> io::Result<Vec<u8>> {
    // STORAGE_PROPERTY_QUERY: PropertyId, PropertyStandardQuery (0), zero additional bytes.
    let input = [property, 0u32, 0u32];
    let mut output = vec![0; MAX_REPLY_BYTES];
    let mut returned = 0;
    // SAFETY: the file owns the handle. Both buffers and the byte count remain live for
    // the synchronous call, and the buffer lengths describe their allocated storage.
    let ok = unsafe {
        DeviceIoControl(
            disk.as_raw_handle(),
            IOCTL_STORAGE_QUERY_PROPERTY,
            input.as_ptr().cast(),
            std::mem::size_of_val(&input) as u32,
            output.as_mut_ptr().cast(),
            output.len() as u32,
            &mut returned,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    if returned as usize > output.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid bytes-returned count",
        ));
    }
    output.truncate(returned as usize);
    Ok(output)
}

fn display(property: u32, bytes: &[u8]) -> ParseResult<()> {
    match property {
        0 => print_serial("SerialNumberOffset", descriptor::device_serial(bytes)?),
        2 => print_ids(&descriptor::device_ids(bytes)?),
        3 => {
            let duid = descriptor::duid(bytes)?;
            print_serial("Embedded device serial", duid.serial);
            if let Some(ids) = duid.identifiers {
                print_ids(&ids);
            } else {
                println!("  No embedded device ID descriptor");
            }
            match duid.layout {
                Some(LayoutSignature::Mbr(signature)) => {
                    println!("  MBR signature: {signature:08X}")
                }
                Some(LayoutSignature::Gpt(guid)) => {
                    let [a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p] = *guid;
                    println!("  GPT disk ID: {d:02X}{c:02X}{b:02X}{a:02X}-{f:02X}{e:02X}-{h:02X}{g:02X}-{i:02X}{j:02X}-{k:02X}{l:02X}{m:02X}{n:02X}{o:02X}{p:02X}");
                }
                None => println!("  No embedded layout signature"),
            }
        }
        _ => return Err("unknown property"),
    }
    Ok(())
}

fn print_serial(label: &str, serial: Option<&[u8]>) {
    if let Some(bytes) = serial {
        println!("  {label}: \"{}\"", bytes.escape_ascii());
        println!("  Serial bytes (hex): {}", hex(bytes));
    } else {
        println!("  {label}: absent");
    }
}

fn print_ids(ids: &[Identifier<'_>]) {
    if ids.is_empty() {
        println!("  No storage identifiers");
    }
    for (index, id) in ids.iter().enumerate() {
        let code_set = match id.code_set {
            1 => "Binary",
            2 => "ASCII",
            3 => "UTF-8",
            _ => "Unknown",
        };
        let kind = match id.kind {
            0 => "Vendor specific",
            1 => "Vendor ID",
            2 => "EUI designator",
            3 => "FCPH name",
            8 => "SCSI name string",
            _ => "Other",
        };
        let association = match id.association {
            0 => "Device",
            1 => "Port",
            2 => "Target",
            _ => "Unknown",
        };
        println!(
            "  ID {index}: {code_set} ({}), {kind} ({}), {association} ({}), {} bytes",
            id.code_set,
            id.kind,
            id.association,
            id.bytes.len()
        );
        if id.code_set == 2 && id.bytes.is_ascii() {
            println!("    Text: \"{}\"", id.bytes.escape_ascii());
        } else if id.code_set == 3 {
            match std::str::from_utf8(id.bytes) {
                Ok(text) => println!("    Text: {text:?}"),
                Err(_) => println!("    Invalid UTF-8"),
            }
        }
        println!("    Raw bytes (hex): {}", hex(id.bytes));
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut result = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 0x0F) as usize] as char);
    }
    result
}
