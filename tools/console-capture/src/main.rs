#![no_std]
#![no_main]

use console_capture::{PAGE, image_range, read_range, readable};
use core::{
    fmt::{self, Write},
    panic::PanicInfo,
    ptr,
    sync::atomic::Ordering,
};
use xbox360_sys::{status::NtStatus, xam, xboxkrnl as k};

xbox360_sys::bridged!(#[unsafe(no_mangle)] pub _start, title_main);

struct Text {
    bytes: [u8; 512],
    len: usize,
}
impl Text {
    fn new() -> Self {
        Self {
            bytes: [0; 512],
            len: 0,
        }
    }
    fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}
impl Write for Text {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(fmt::Error)?;
        self.bytes
            .get_mut(self.len..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

struct File(Option<k::Handle>);
impl File {
    fn create(path: &[u8], directory: bool) -> Result<Self, NtStatus> {
        let name = k::AnsiString::new(path).ok_or(NtStatus(0xc000000d))?;
        let attrs = k::ObjectAttributes {
            root_directory: k::Handle::NULL,
            object_name: &name,
            attributes: k::ObjectAttributeFlags::CASE_INSENSITIVE,
        };
        let mut handle = k::Handle::NULL;
        let mut io = k::IoStatusBlock::default();
        let status = unsafe {
            k::NtCreateFile(
                &mut handle,
                k::AccessMask::GENERIC_WRITE | k::AccessMask::SYNCHRONIZE,
                &attrs,
                &mut io,
                ptr::null(),
                if directory {
                    k::FileAttributes::DIRECTORY
                } else {
                    k::FileAttributes::NORMAL
                },
                k::ShareAccess::READ,
                k::CreateDisposition::Create,
                k::CreateOptions::SYNCHRONOUS_IO_NONALERT
                    | if directory {
                        k::CreateOptions::DIRECTORY_FILE
                    } else {
                        k::CreateOptions::NON_DIRECTORY_FILE
                    },
            )
        };
        if status != NtStatus::SUCCESS {
            return Err(status);
        }
        Ok(Self(Some(handle)))
    }
    fn write(&self, mut bytes: &[u8]) -> bool {
        while !bytes.is_empty() {
            let mut io = k::IoStatusBlock::default();
            let status = unsafe {
                k::NtWriteFile(
                    self.0.unwrap(),
                    k::Handle::NULL,
                    0,
                    0,
                    &mut io,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    ptr::null(),
                )
            };
            let n = io.information as usize;
            if status != NtStatus::SUCCESS
                || io.status != NtStatus::SUCCESS
                || n == 0
                || n > bytes.len()
            {
                return false;
            }
            bytes = &bytes[n..];
        }
        true
    }
    fn flush(&self) -> bool {
        let mut io = k::IoStatusBlock::default();
        unsafe {
            k::NtFlushBuffersFile(self.0.unwrap(), &mut io) == NtStatus::SUCCESS
                && io.status == NtStatus::SUCCESS
        }
    }
    fn close(mut self) -> bool {
        let flushed = self.flush();
        let closed = unsafe { k::NtClose(self.0.take().unwrap()) == NtStatus::SUCCESS };
        flushed && closed
    }
}
impl Drop for File {
    fn drop(&mut self) {
        if let Some(h) = self.0.take() {
            unsafe {
                k::NtClose(h);
            }
        }
    }
}

struct Capture {
    root: Text,
    log: File,
}
impl Capture {
    fn new() -> Option<Self> {
        for n in 0..100 {
            let mut root = Text::new();
            write!(root, "\\??\\game:\\CAP{n:02}").ok()?;
            match File::create(root.bytes(), true) {
                Ok(dir) => {
                    drop(dir);
                }
                Err(NtStatus(0xc0000035)) => continue,
                Err(_) => return None,
            }
            let mut path = Text::new();
            write!(
                path,
                "{}\\report.txt",
                core::str::from_utf8(root.bytes()).ok()?
            )
            .ok()?;
            let log = File::create(path.bytes(), false).ok()?;
            return Some(Self { root, log });
        }
        None
    }
    fn file(&self, name: &str) -> Option<File> {
        let mut path = Text::new();
        write!(
            path,
            "{}\\{name}",
            core::str::from_utf8(self.root.bytes()).ok()?
        )
        .ok()?;
        File::create(path.bytes(), false).ok()
    }
    fn note(&self, args: fmt::Arguments<'_>) -> bool {
        let mut t = Text::new();
        t.write_fmt(args).is_ok() && self.log.write(t.bytes()) && self.log.flush()
    }
}
macro_rules! note { ($c:expr, $($arg:tt)*) => { $c.note(format_args!($($arg)*)) }; }

// This is a best-effort read of live image pages, not an atomic machine snapshot.
fn copy_checked(address: u32, output: &mut [u8]) -> bool {
    if !read_range(address, output.len()) {
        return false;
    }
    let mut done = 0;
    while done < output.len() {
        let first = address + done as u32;
        let count = (PAGE - first as usize % PAGE).min(output.len() - done);
        if !k::MmIsAddressValid(first).get()
            || !k::MmIsAddressValid(first + count as u32 - 1).get()
            || !readable(k::MmQueryAddressProtect(first).bits())
        {
            return false;
        }
        for i in 0..count {
            output[done + i] = unsafe { ptr::read_volatile((first as usize + i) as *const u8) };
        }
        done += count;
    }
    true
}

fn module(c: &Capture, name: &core::ffi::CStr, stem: &str) -> bool {
    let mut handle = 0;
    let status = unsafe { k::XexGetModuleHandle(name.as_ptr().cast(), &mut handle) };
    if !note!(
        c,
        "module={stem} lookup={:08x} handle={handle:08x}\n",
        status.0
    ) {
        return false;
    }
    if status != NtStatus::SUCCESS || handle == 0 {
        return true;
    }
    let mut ldr = [0u8; 0x60];
    if !copy_checked(handle, &mut ldr) {
        return note!(c, "module={stem} skipped=unreadable_loader_entry\n");
    }
    let base = u32::from_be_bytes(ldr[0x18..0x1c].try_into().unwrap());
    let size = u32::from_be_bytes(ldr[0x20..0x24].try_into().unwrap());
    if !image_range(base, size) {
        return note!(
            c,
            "module={stem} skipped=invalid_image_range base={base:08x} size={size:08x}\n"
        );
    }
    if !note!(c, "module={stem} base={base:08x} size={size:08x} begin\n") {
        return false;
    }
    let mut file_name = Text::new();
    if write!(file_name, "{stem}.mem").is_err() {
        return false;
    }
    let Some(image) = c.file(core::str::from_utf8(file_name.bytes()).unwrap()) else {
        return false;
    };
    let mut map_name = Text::new();
    if write!(map_name, "{stem}.pages").is_err() {
        return false;
    }
    let Some(map) = c.file(core::str::from_utf8(map_name.bytes()).unwrap()) else {
        return false;
    };
    let mut offset = 0u32;
    let mut copied = 0u32;
    let mut scratch = [0u8; PAGE];
    while offset < size {
        let length = (size - offset).min(PAGE as u32) as usize;
        let address = base + offset;
        let protection = if k::MmIsAddressValid(address).get() {
            k::MmQueryAddressProtect(address).bits()
        } else {
            0
        };
        scratch.fill(0);
        let count = if copy_checked(address, &mut scratch[..length]) {
            length as u32
        } else {
            scratch.fill(0);
            0
        };
        let mut record = [0u8; 12];
        record[..4].copy_from_slice(&offset.to_be_bytes());
        record[4..8].copy_from_slice(&protection.to_be_bytes());
        record[8..].copy_from_slice(&count.to_be_bytes());
        if !image.write(&scratch[..length]) || !map.write(&record) {
            return false;
        }
        copied += count;
        offset += length as u32;
    }
    let closed_image = image.close();
    let closed_map = map.close();
    if !closed_image || !closed_map {
        return false;
    }
    let mut ldr_name = Text::new();
    if write!(ldr_name, "{stem}.ldr").is_err() {
        return false;
    }
    let Some(ldr_file) = c.file(core::str::from_utf8(ldr_name.bytes()).unwrap()) else {
        return false;
    };
    if !ldr_file.write(&ldr) || !ldr_file.close() {
        return false;
    }
    note!(
        c,
        "module={stem} completed bytes={size} copied={copied} skipped={}\n",
        size - copied
    )
}

fn capture(c: &Capture) -> bool {
    if !note!(
        c,
        "xb-emu console-capture v2\nenvironment=running-title NOT early-boot\nnot_atomic=true\nscope=selected_loaded_images hardware memory_statistics smc_queries\npages_format=big_endian_u32_offset,u32_protection,u32_bytes_copied; stride=4096\nskipped_pages_are_zero_filled_NOT_measured_zeroes\n"
    ) {
        return false;
    }
    if let Some(v) = k::XboxKrnlVersion.get() {
        if !note!(c, "kernel={v}\n") {
            return false;
        }
    }
    if let Some(v) = k::XboxKrnlBaseVersion.get() {
        if !note!(c, "base_kernel={v}\n") {
            return false;
        }
    }
    if let Some(h) = k::XboxHardwareInfo.get() {
        if !note!(
            c,
            "hardware_flags={:08x} boot_flags={:04x}\n",
            h.flags.load(Ordering::Relaxed),
            h.boot_flags.bits()
        ) {
            return false;
        }
    }
    let mut stats = k::MmStatistics::new();
    let status = unsafe { k::MmQueryStatistics(&mut stats) };
    if !note!(
        c,
        "memory_statistics_result={:08x} physical_pages={} kernel_pages={} title_image_pages={} system_image_pages={}\n",
        status.0,
        stats.total_physical_pages,
        stats.kernel_pages,
        stats.title.image_pages,
        stats.system.image_pages
    ) {
        return false;
    }
    if status == NtStatus::SUCCESS {
        let Some(out) = c.file("memory-statistics.bin") else {
            return false;
        };
        let bytes = unsafe {
            core::slice::from_raw_parts(
                ptr::from_ref(&stats).cast::<u8>(),
                core::mem::size_of_val(&stats),
            )
        };
        if !out.write(bytes) || !out.close() {
            return false;
        }
    }
    // Do not load a missing module: changing the running title would invalidate the reference.
    for (name, stem) in [
        (c"xboxkrnl.exe", "xboxkrnl"),
        (c"xam.xex", "xam"),
        (c"launch.xex", "launch"),
        (c"lhelper.xex", "lhelper"),
        (c"Proto.xex", "proto"),
        (c"GoldSpoof.xex", "goldspoof"),
        (c"GoldSpoof17559.xex", "goldspoof17559"),
        (c"UsbdSecPatch.xex", "usbdsecpatch"),
        (c"xbdm.xex", "xbdm"),
        (c"hud.xex", "hud"),
        (c"dash.xex", "dash"),
        (c"Aurora.xex", "aurora"),
    ] {
        if !module(c, name, stem) {
            return false;
        }
    }
    // Synchronous SMC queries run last; a stalled call leaves the images and a checkpoint on disk.
    for command in [0x12, 0x07, 0x0a, 0x0f, 0x17] {
        for sample in 0..3 {
            if !note!(
                c,
                "smc command={command:02x} sample={sample} awaiting_reply\n"
            ) {
                return false;
            }
            let mut tx = k::SmcMessage([0; 16]);
            tx.0[0] = command;
            let mut rx = k::SmcMessage([0; 16]);
            let result = unsafe { k::HalSendSMCMessage(&tx, &mut rx) };
            let mut line = Text::new();
            if write!(
                line,
                "smc command={command:02x} sample={sample} hal_result={result:08x} rx="
            )
            .is_err()
            {
                return false;
            }
            for byte in rx.0 {
                if write!(line, "{byte:02x}").is_err() {
                    return false;
                }
            }
            if writeln!(line).is_err() || !c.log.write(line.bytes()) || !c.log.flush() {
                return false;
            }
        }
    }
    note!(c, "capture_complete\n")
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    unsafe {
        k::DbgPrint(
            c"console-capture: panic; preserve partial CAP folder\n"
                .as_ptr()
                .cast(),
        );
        xam::XamLoaderLaunchTitle(ptr::null(), 0)
    }
}
extern "C" fn title_main(_context: u32) -> u32 {
    if let Some(c) = Capture::new() {
        let ok = capture(&c);
        if !ok {
            note!(&c, "capture_incomplete\n");
        }
        unsafe {
            k::DbgPrint(
                if ok {
                    c"console-capture: complete\n"
                } else {
                    c"console-capture: incomplete; preserve partial CAP folder\n"
                }
                .as_ptr()
                .cast(),
            );
        }
    } else {
        unsafe {
            k::DbgPrint(
                c"console-capture: cannot create CAP folder\n"
                    .as_ptr()
                    .cast(),
            );
        }
    }
    unsafe { xam::XamLoaderLaunchTitle(ptr::null(), 0) }
}
