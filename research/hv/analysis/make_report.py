sec = open(r'C:\Users\Luan Bogo\xb-emu\research\hv\analysis\secfunc_84e0.txt').read()
report = '''# Xbox 360 Hypervisor (17559) — Security Engine / Crypto Analysis

Binary: `dumps/console/hypervisor_17559.bin` (65536 bytes)
CPU key at file offset 0x20: `5cb2...6f78 (redacted; ver dumps/console/)`

## Method

- Disassembled with Capstone (`CS_ARCH_PPC`, `CS_MODE_64 | CS_MODE_BIG_ENDIAN`).
- Recursive descent from the reset vector (0x100) plus hypercall dispatch
  tables (file offsets 0x80/0xae80) to reach code not directly branched to.
- Searched for offset-0x20 references, high/SoC MMIO addresses, 10-16
  iteration loops, and constants (0x1b, 0x63, 0x9e3779b9). No AES S-box or
  TEA constants are present; AES is implemented in hardware, not in this HV.

## 1. Functions referencing offset 0x20 (CPU-key-adjacent)

The well-known 128-byte block copy at `0xe14` loads/stores `0x20(r3)` /
`0x20(r4)` as part of copying 16 qwords. Other code that touches displacement
`0x20` of a structure:

| Address | Context |
|---|---|
| `0x202c` | `lwz r10, 0x20(r3)` compared against `0x42440302` (device/revision check) |
| `0x21ac`, `0x21c4` | `ld r11, 0x20(r31)` (16-byte copy into a buffer) |
| `0x3e08` | `ld r11, 0x20(r11)` (linked-list / table entry) |
| `0x6a18` | `std r29, 0x20(r11)` (store into a 0x40-byte record) |
| `0x85d4`, `0x884c` | `std r10, 0x20(r11)` (security-engine register save/restore, see section 3) |
| `0x8fd0`, `0x8fd8` | `std/ld r11, 0x20(r18)` (security-engine register clear/poll) |
| `0xa9b8`, `0xa9cc` | `ld/std r6, 0x20(r4/r3)` (generic 8-qword block copy) |

The CPU key itself (0x20..0x2f of the HV header) is consumed by the
`0xe14` copy and by the security-engine setup in section 3; no other routine
reads the raw 16 bytes directly.

## 2. High / SoC addresses accessed

Beyond the usual `0x8000_xxxx` low memory, the HV programs memory-mapped
I/O at these bases:

- `0x8000020000000000` - SoC base used for `lis 2/4/6; bl 0x6a0`.
- `0x8000020000040000` (I/O block `+0x6000/+0x6100`) - used at `0x3088`.
- `0x8000020000050000` (I/O block `+0x6000..+0x6040`) - used by `0x84e0`.
- `0x8000020000060000` (I/O block `+0x1a0`, `+0x1c8`) - used by `0x84e0`.
- `0x80000200ea000000` - IIC (interrupt controller), programmed at `0xb510`
  (`stw` to `+0x1014`).
- The documented security-engine block is `0x24000` relative to SoC; the HV
  reaches it through the `0x84e0` save/restore path rather than by a literal
  `0x24000` immediate.

## 3. Candidate security-engine (memory encryption) routine - 0x84e0

Called from `0x8b40`, `0x9370`, `0xa458` (`bl 0x84e0`); paired with
`0x87a8` (restore). It saves the five security-engine registers
(`0x6000..0x6040`) into a global at `0x6360(r2)`, clears them, waits for
completion, then later restores them. This is the routine that configures
the memory-encryption engine during boot and on world switches.

Full disassembly:

```
''' + sec + '''
```

Paired restore routine `0x87a8` (excerpt) loads the five saved values back
from `0x6360(r2)` into `0x6000..0x6040(r3)` and polls for completion.

## 4. Loops of 10-16 iterations

Static scan found the following `mtctr` loops with counters in [8,20]. None
contain AES-round constants or S-boxes; they are byte copies / checksums
/ MMIO polling, not software crypto:

| Address | Counter | Purpose (from context) |
|---|---|---|
| `0x216c` | 16 | copy 16 bytes into buffer |
| `0x3cd4` | 16 | table copy |
| `0x3f34` | 16 | table copy |
| `0x502c` | 16 | table copy |
| `0x53c4` | 16 | table copy |
| `0x54d0` | 16 | table copy |
| `0x5aa0` | 16 | table copy |
| `0x5ae8` | 16 | table copy |
| `0x6ec8` | 10 | copy 10 bytes |
| `0x9788`, `0x9928`, `0x9a78`, `0x9d18`, `0xa088`, `0xa14c`, `0xa480` | 12 | copy 12 bytes / loop |
| `0x9f3c` | 16 | copy 16 bytes |
| `0x2108` family | 12 | buffer fill |
| `0x42a0`, `0x7104` | 20 | larger copy |

## 5. Call graph from the reset vector (0x100)

```
0x100 (system reset)
  -> 0x1870 (primary init; only if SPR 0x131 == 0)
       0x1880: bla 0xb510   (IIC init: writes 0xea00/0x2000000000000000)
       0x1884: bl  0x3c4     (dcache/line zero, r4 blocks)
       0x188c: bl  0x2e8     (returns address 0x6a00000001f60000)
       0x1890: bl  0x3c4
       0x1898: ... (MMU/SLB setup)
       0x1918: ... (sets SDR1/PCR at 0x130/0x131)
       0x1938: bl  0x2e8
       0x1944: mtspr 0x131, r3   (publish PCR)
       0x1958: beql 0x364      (secondary-thread path)
       0x195c: slbia / SLB load loop (0x1964..)
  secondary path 0x364: bl 0x218 (enter HV mode / disable interrupts)
```

Hypercall entry (`0x77c`, table at `0x80`) dispatches through `0xae80`
to handlers in the `0x0xxx`/`0x2xxx` (file offset) ranges; security-related
hypercalls land in the `0x84e0`/`0x87a8` pair and the `0xa458` setup.

## 6. Suspicious constants / patterns

- `0x42440302` at `0x2034` - compared against a word at `+0x20` of a device
  structure (likely a device-ID / revision check).
- `0x71700` at `0x2c1c` - compared against a counter at `0x6aa0(r10)`
  (version / revision gate).
- `0xc0` (IIC vector base) written at `0x1910`.
- `0x1010`/`0x1014` IIC register offsets.
- `0x6360(r2)` global - 5-word save area for the security-engine registers.
- No `0x1b`/`0x63`/`0x9e3779b9` immediates and no AES S-box in the image;
  crypto is delegated to the security engine hardware via `0x84e0`.
'''
open(r'C:\Users\Luan Bogo\xb-emu\research\hv\hypervisor_analysis.md','w').write(report)
print('written')