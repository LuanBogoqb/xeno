#![no_std]
#![no_main]

use core::{panic::PanicInfo, ptr};
use smc_capture::Record;
use xbox360_sys::{xam, xboxkrnl as k};

xbox360_sys::bridged!(#[unsafe(no_mangle)] pub _start, title_main);

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    unsafe { k::DbgPrint(c"smc-capture: panic; inspect partial log\n".as_ptr().cast()) };
    unsafe { xam::XamLoaderLaunchTitle(ptr::null(), 0) }
}

fn open_log() -> Option<k::Handle> {
    // game: is the directory from which Aurora launched this title. CREATE never overwrites a log.
    let mut path = *b"\\??\\game:\\SMC-00.TXT";
    for n in 0..100 {
        let digits = path.len() - 6;
        path[digits] = b'0' + n / 10;
        path[digits + 1] = b'0' + n % 10;
        let name = k::AnsiString::new(&path)?;
        let attrs = k::ObjectAttributes {
            root_directory: k::Handle::NULL,
            object_name: &name,
            attributes: k::ObjectAttributeFlags::CASE_INSENSITIVE,
        };
        let mut handle = k::Handle::NULL;
        let mut iosb = k::IoStatusBlock::default();
        let status = unsafe {
            k::NtCreateFile(
                &mut handle,
                k::AccessMask::GENERIC_WRITE | k::AccessMask::SYNCHRONIZE,
                &attrs,
                &mut iosb,
                ptr::null(),
                k::FileAttributes::NORMAL,
                k::ShareAccess::READ,
                k::CreateDisposition::Create,
                k::CreateOptions::SYNCHRONOUS_IO_NONALERT | k::CreateOptions::NON_DIRECTORY_FILE,
            )
        };
        if status == xbox360_sys::status::NtStatus::SUCCESS {
            return Some(handle);
        }
        // Only retry a name collision; a missing/read-only game: is a real error.
        if status.0 != 0xc0000035 {
            break;
        }
    }
    None
}

fn append(handle: k::Handle, bytes: &[u8]) -> bool {
    let mut iosb = k::IoStatusBlock::default();
    let status = unsafe {
        k::NtWriteFile(
            handle,
            k::Handle::NULL,
            0,
            0,
            &mut iosb,
            bytes.as_ptr(),
            bytes.len() as u32,
            ptr::null(),
        )
    };
    if status != xbox360_sys::status::NtStatus::SUCCESS
        || iosb.status != xbox360_sys::status::NtStatus::SUCCESS
        || iosb.information != bytes.len() as u32
    {
        return false;
    }
    unsafe { k::NtFlushBuffersFile(handle, &mut iosb) == xbox360_sys::status::NtStatus::SUCCESS }
}

extern "C" fn title_main(_ctx: u32) -> u32 {
    if let Some(handle) = open_log() {
        let mut ok = append(handle, b"xb-emu smc-capture v1\nenvironment=running-title NOT early-boot\ncommand=12 count=3\n");
        for sample in 0..3 {
            if !ok {
                break;
            }
            // Flush before calling the synchronous kernel API, so a stalled call leaves evidence.
            if !append(
                handle,
                b"tx=12000000000000000000000000000000 awaiting_reply\n",
            ) {
                ok = false;
                break;
            }
            let mut tx = k::SmcMessage([0; 16]);
            tx.0[0] = 0x12;
            let mut rx = k::SmcMessage([0; 16]);
            let status = unsafe { k::HalSendSMCMessage(&tx, &mut rx) };
            ok = append(handle, Record::reply(sample, status, &rx.0).as_bytes());
        }
        if ok {
            ok = append(handle, b"capture_complete\n");
        }
        let closed = unsafe { k::NtClose(handle) };
        let message = if ok && closed == xbox360_sys::status::NtStatus::SUCCESS {
            c"smc-capture: complete; SMC-NN.TXT next to default.xex\n"
        } else {
            c"smc-capture: file I/O failed; capture is incomplete\n"
        };
        unsafe { k::DbgPrint(message.as_ptr().cast()) };
    } else {
        unsafe {
            k::DbgPrint(
                c"smc-capture: cannot create log; launch from writable USB\n"
                    .as_ptr()
                    .cast(),
            )
        };
    }
    unsafe { xam::XamLoaderLaunchTitle(ptr::null(), 0) }
}
