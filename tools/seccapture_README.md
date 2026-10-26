# seccapture — Dashlaunch kernel plugin

Captures Xenon memory that a user-mode title cannot reach (security-engine
regions, physical backing) using the freeboot hypervisor memcpy.

## Build

```
tools\build_seccapture.bat
```

Produces `tools/SecCapture.xex` (XEXDLL, `<sysdll/>`, base `0x91100000`).
Requires the Xbox 360 XDK (`XEDK` or `C:\Users\Luan Bogo\xdk`).

## Deploy / recovery

- Put `SecCapture.xex` at `Hdd1:\Plugins\SecCapture.xex`.
- Enable it only from a `launch.ini` on the **USB stick**:
  `plugin4 = Hdd:\Plugins\SecCapture.xex`.
- Keep the **HDD `launch.ini` clean** (`plugin4` empty). The USB ini is read
  first (device order `Usb0/1/2` before `Hdd`), so:
  - USB present -> plugin loads;
  - if the plugin hangs, **pull the USB and reboot** -> the clean HDD config
    boots normally.
- Alternatively boot **Xell** (Eject button) if needed.

## Critical gotchas (learned the hard way)

- **`HvxMemcpy` only works from `DllMain`.** Calling it from a system thread
  (`ExCreateThread` + `XapiThreadStartup`) hangs the console.
- **File I/O works from `DllMain`** when the path uses the Dashlaunch mount
  (`dlaunch:\Plugins\seccap.bin`). `\??\hdd1:` and `\Device\...` failed.
- **Chunk size `0x400` is safe; `0x1000` hung** on this console.
- Region **3 (encrypted) hangs** the hypercall — do not put it before data you
  want to keep.
- Records are flushed as written, so a later hang leaves earlier records on
  disk.

## Memory regions

The source real address encodes the region in bits 40..43:

| region | meaning |
| --- | --- |
| 0 | physical (ciphertext) |
| 1 | hashed / decrypted (kernel plaintext) |
| 2 | SoC (security engine, OTP, ...) |
| 3 | encrypted (hangs) |

## Output

`dlaunch:\Plugins\seccap.bin`:

```
u32 magic 'SECC'
u32 version
u32 kernel build
u32 chunk count
repeated: u32 tag (0x3000|region), u32 offset, u32 len, u8 data[len]
```

Parse with `python tools/parse_seccap3.py dumps/console/seccap.bin`.

## What the 2026-10-25 run captured

- 4 KiB plaintext/ciphertext pair at `0x3B80..0x4B80` (region 1 vs region 0).
  Region 1 is byte-identical to `dumps/stages/kernel_17559.bin`; region 0 is
  ciphertext. Saved as `dumps/console/sec_pair_0x3b80.bin`.
- SECENG key block at `0x24000` reads `0xffffffff` (write-only/masked).
- OTP block at `0x20000` contains the CPU key.
- Region 3 hangs the hypercall.
- `MmMapIoSpace` from the guest kernel returns NULL for the SECENG SoC block.

The real cipher is chained/tweakable (XTS/XEX-like), not ECB and not a
repeating XOR stream. Implementing it (cipher + CPU-key KDF) is still pending;
the pair above is the oracle.
