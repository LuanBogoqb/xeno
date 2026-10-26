#include <xtl.h>
#define SECC_MAGIC 0x53454343u
#define SECC_VERSION 25u

typedef struct _XBOX_KRNL_VERSION { WORD Major, Minor, Build, Qfe; } XBOX_KRNL_VERSION, *PXBOX_KRNL_VERSION;

// Record header (big-endian, 16 bytes): tag, window, offset, len.
//   tag    = 0x3000 | region   0 = physical/ciphertext, 1 = HV/physical decrypted, 2 = SoC MMIO
//   window = block index; block k lives at 0x8000010000000000 + k*0x200000000
typedef struct _SEC_CHUNK { unsigned int region, window, offset, len; } SEC_CHUNK;

extern "C" {
    VOID DbgPrint(const char* fmt, ...);
    DWORD MmGetPhysicalAddress(void* buffer);
    extern PXBOX_KRNL_VERSION XboxKrnlVersion;
}

static int __declspec(naked) HvxMemcpy(unsigned int magic, int op, unsigned long long src,
                                       unsigned long long dst, unsigned long long len) {
    __asm { li r0, 0; sc; blr }
}

#define HV_MAGIC 0x72627472u
#define HV_OP_MEMCPY 5
#define HV_REGION 0x8000000000000000ULL
#define HV_BLOCK_STRIDE 0x200000000ULL
#define STEP 0x1000u

__declspec(align(128)) static unsigned char g_buf[STEP];

// v25: full 256 KiB hypervisor dump plus the same-boot ciphertext pair, so the
// memory key can be searched for inside the HV image itself.  The pair must come
// from the same boot as the dump: the DRAM keys are regenerated every boot
// (three captures of 0x3B80 across boots differ while the plaintext is equal).
static const SEC_CHUNK g_chunks[] = {
    { 0, 0, 0x3B80, 0x1000 },   // ciphertext half of the pair (physical view)
    { 1, 0, 0x0000, 0x10000 },  // HV image block 0 (also holds the plaintext half at 0x3B80)
    { 1, 1, 0x10000, 0x10000 }, // HV image block 1 (syscall table, key save area)
    { 1, 2, 0x20000, 0x10000 }, // HV image block 2
    { 1, 3, 0x30000, 0x10000 }, // HV image block 3
    { 2, 0, 0x10000, 0x400 },   // SE area that reads back high-entropy data
    { 2, 0, 0x56000, 0x40 },    // SE control registers
};
#define NCHUNKS (sizeof(g_chunks) / sizeof(g_chunks[0]))

static HANDLE open_output(void) {
    return CreateFileA("dlaunch:\\Plugins\\seccap.bin", GENERIC_WRITE,
                       FILE_SHARE_WRITE, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
}

static BOOL write_all(HANDLE h, const void* d, DWORD l) {
    DWORD w = 0;
    if (!WriteFile(h, d, l, &w, NULL) || w != l) return FALSE;
    FlushFileBuffers(h);
    return TRUE;
}

static BOOL capture_chunk(HANDLE h, const SEC_CHUNK* c) {
    unsigned int off = c->offset;
    unsigned int left = c->len;
    while (left) {
        unsigned int step = left < STEP ? left : STEP;
        DWORD phys = MmGetPhysicalAddress(g_buf);
        unsigned long long dst = HV_REGION | (unsigned long long)phys;
        unsigned long long src = HV_REGION | ((unsigned long long)c->region << 40)
                               | ((unsigned long long)c->window * HV_BLOCK_STRIDE)
                               | (unsigned long long)off;
        unsigned int hdr[4] = { 0x3000u | c->region, c->window, off, step };
        memset(g_buf, 0, STEP);
        if (!write_all(h, hdr, sizeof(hdr))) return FALSE;
        HvxMemcpy(HV_MAGIC, HV_OP_MEMCPY, src, dst, step);
        if (!write_all(h, g_buf, step)) return FALSE;
        off += step;
        left -= step;
    }
    return TRUE;
}

BOOL APIENTRY DllMain(HANDLE h, DWORD reason, LPVOID reserved) {
    (void)h; (void)reserved;
    if (reason != DLL_PROCESS_ATTACH) return TRUE;
    __try {
        HANDLE f = open_output();
        if (f != INVALID_HANDLE_VALUE) {
            unsigned int hdr[4] = {
                SECC_MAGIC, SECC_VERSION,
                (XboxKrnlVersion ? XboxKrnlVersion->Build : 0), NCHUNKS
            };
            if (write_all(f, hdr, sizeof(hdr))) {
                for (unsigned i = 0; i < NCHUNKS; i++)
                    if (!capture_chunk(f, &g_chunks[i])) break;
            }
            CloseHandle(f);
        }
    } __except (1) {}
    return TRUE;
}
