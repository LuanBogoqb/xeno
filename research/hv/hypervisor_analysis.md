# Xbox 360 Hypervisor (17559) � Security Engine / Crypto Analysis

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
84e0: mflr r12
84e4: bl 0xa808
84e8: stdu r1, -0x80(r1)
84ec: mfspr r30, 0x131
84f0: lbz r11, 0x81(r30)
84f4: cmplwi cr6, r11, 0
84f8: beq cr6, 0x8500
84fc: bl 0x218
8500: cmplwi cr6, r3, 2
8504: ble cr6, 0x850c
8508: bl 0x218
850c: stb r3, 0x82(r30)
8510: sync 
8514: li r11, 1
8518: stb r11, 0x81(r30)
851c: sync 
8520: lis r3, 6
8524: ori r3, r3, 0x1000
8528: bl 0x6a0
852c: mr r29, r3
8530: lis r3, 5
8534: bl 0x6a0
8538: lbz r11, 0x80(r30)
853c: rldicr r10, r11, 0xc, 0x3f
8540: cmplwi cr6, r11, 0
8544: add r28, r10, r3
8548: bne cr6, 0x86f4
854c: addis r11, r30, 1
8550: li r10, 5
8554: addi r11, r11, -0x5f7f
8558: b 0x8560
855c: mr r31, r31
8560: lbz r9, 0(r11)
8564: cmplwi cr6, r9, 1
8568: bne cr6, 0x855c
856c: addi r10, r10, -1
8570: addi r11, r11, -0x2000
8574: cmplwi cr6, r10, 0
8578: bne cr6, 0x8560
857c: ld r11, 0x1a0(r29)
8580: li r12, -0x101
8584: and r11, r11, r12
8588: std r11, 0x1a0(r29)
858c: eieio 
8590: ld r11, 0x1a0(r29)
8594: cmpldi cr6, r11, 0
8598: beq cr6, 0x85a0
859c: nop 
85a0: isync 
85a4: addis r11, r2, 1
85a8: ld r10, 0x6000(r3)
85ac: li r31, 0
85b0: addi r11, r11, 0x6360
85b4: std r10, 0(r11)
85b8: ld r10, 0x6010(r3)
85bc: std r10, 8(r11)
85c0: ld r10, 0x6020(r3)
85c4: std r10, 0x10(r11)
85c8: ld r10, 0x6030(r3)
85cc: std r10, 0x18(r11)
85d0: ld r10, 0x6040(r3)
85d4: std r10, 0x20(r11)
85d8: std r31, 0x6000(r3)
85dc: eieio 
85e0: std r31, 0x6010(r3)
85e4: eieio 
85e8: std r31, 0x6020(r3)
85ec: eieio 
85f0: std r31, 0x6030(r3)
85f4: eieio 
85f8: std r31, 0x6040(r3)
85fc: eieio 
8600: ld r10, 0x6040(r3)
8604: cmpldi cr6, r10, 0
8608: beq cr6, 0x8610
860c: nop 
8610: isync 
8614: ld r10, 0x1c8(r29)
8618: std r10, 0x28(r11)
861c: std r31, 0x1c8(r29)
8620: eieio 
8624: ld r11, 0x1c8(r29)
8628: cmpldi cr6, r11, 0
862c: beq cr6, 0x8634
8630: nop 
8634: isync 
8638: li r10, 5
863c: li r11, 0
8640: rldimi r11, r10, 0xd, 0x13
8644: add r11, r11, r30
8648: lbz r9, 0x82(r11)
864c: lbz r8, 0x82(r30)
8650: cmplw cr6, r9, r8
8654: bne cr6, 0x86ec
8658: li r9, 2
865c: stb r9, 0x81(r11)
8660: sync 
8664: b 0x866c
8668: mr r31, r31
866c: lbz r9, 0x81(r11)
8670: cmplwi cr6, r9, 3
8674: bne cr6, 0x8668
8678: addi r10, r10, -1
867c: cmplwi cr6, r10, 0
8680: bne cr6, 0x863c
8684: bl 0x4cc
8688: li r11, 0x200
868c: mr r10, r31
8690: lbz r9, 0(r3)
8694: addi r11, r11, -1
8698: addi r3, r3, 0x80
869c: add r10, r9, r10
86a0: cmplwi cr6, r11, 0
86a4: bne cr6, 0x8690
86a8: cmpldi cr6, r10, 0
86ac: beq cr6, 0x86b4
86b0: nop 
86b4: isync 
86b8: li r11, 0x180
86bc: mr r10, r30
86c0: lbz r9, 0(r10)
86c4: addi r11, r11, -1
86c8: addi r10, r10, 0x80
86cc: add r31, r9, r31
86d0: cmplwi cr6, r11, 0
86d4: bne cr6, 0x86c0
86d8: cmpldi cr6, r31, 0
86dc: beq cr6, 0x86e4
86e0: nop 
86e4: isync 
86e8: b 0x8700
86ec: bl 0x218
86f0: mr r31, r31
86f4: lbz r11, 0x81(r30)
86f8: cmplwi cr6, r11, 2
86fc: bne cr6, 0x86f0
8700: li r11, 0x7c
8704: std r11, 8(r28)
8708: eieio 
870c: ld r11, 8(r28)
8710: cmpldi cr6, r11, 0
8714: beq cr6, 0x871c
8718: nop 
871c: isync 
8720: mfspr r10, 0x16
8724: lis r11, 0x7fff
8728: stw r10, 0x88(r30)
872c: ori r11, r11, 0xffff
8730: mtspr 0x16, r11
8734: lbz r9, 0x80(r30)
8738: clrlwi r10, r9, 0x1f
873c: cmplwi cr6, r10, 0
8740: bne cr6, 0x8768
8744: b 0x874c
8748: mr r31, r31
874c: mfspr r10, 0x88
8750: rlwinm r10, r10, 0, 9, 9
8754: cmpldi cr6, r10, 0
8758: bne cr6, 0x8748
875c: mfspr r10, 0x136
8760: stw r10, 0x8c(r30)
8764: mtspr 0x136, r11
8768: mfspr r11, 0x399
876c: sync 
8770: lis r12, -0x71
8774: nor r10, r9, r9
8778: ori r12, r12, 0xffff
877c: rlwinm r10, r10, 0x14, 0xb, 0xb
8780: and r11, r11, r12
8784: or r11, r10, r11
8788: mtspr 0x399, r11
878c: sync 
8790: isync 
8794: li r11, 3
8798: stb r11, 0x81(r30)
879c: sync 
87a0: addi r1, r1, 0x80
87a4: b 0xa858
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
