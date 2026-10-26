/* capture_all.c
 *
 * Xbox 360 homebrew (XDK 20353) that captures the vectors needed to finish
 * M2 of xb-emu in one run:
 *   - SMC command 0x01 reply
 *   - 256 bytes of region 1 (hashed RAM, 0x0100_0000_0000)
 *   - 256 bytes of physical DRAM (0x8000_0000_0000_0000)
 *
 * Output: capture_all.bin next to the .xex (via \??\game:\...).
 *
 * Build: tools/build_capture_all.bat
 */
#include <xtl.h>
#include <winsockx.h>
#include <stdio.h>
#include <string.h>

#define CAPTURE_NAME "capture_all.bin"

extern DWORD HalSendSMCMessage(const BYTE *request, BYTE *response);

static BOOL write_one(const char *path, const void *buf, DWORD len) {
    HANDLE h;
    DWORD written = 0;
    BOOL ok;
    h = CreateFileA(path, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS,
                    FILE_ATTRIBUTE_NORMAL, NULL);
    if (h == INVALID_HANDLE_VALUE) return FALSE;
    ok = WriteFile(h, buf, len, &written, NULL) && written == len;
    CloseHandle(h);
    return ok;
}

static void write_named_file(const char *name, const void *buf, DWORD len) {
    char path[MAX_PATH];
    _snprintf(path, MAX_PATH, "\\??\\game:\\%s", name);
    if (write_one(path, buf, len)) return;
    _snprintf(path, MAX_PATH, "\\??\\mass0:\\%s", name);
    if (write_one(path, buf, len)) return;
    _snprintf(path, MAX_PATH, "\\??\\hdd1:\\%s", name);
    if (write_one(path, buf, len)) return;
    if (write_one(name, buf, len)) return;
    _snprintf(path, MAX_PATH, "Game:\\%s", name);
    if (write_one(path, buf, len)) return;
    _snprintf(path, MAX_PATH, "Usb0:\\%s", name);
    if (write_one(path, buf, len)) return;
    _snprintf(path, MAX_PATH, "Hdd1:\\%s", name);
    if (write_one(path, buf, len)) return;
}

void __cdecl main(void) {
    BYTE smc_req[16];
    BYTE smc_resp[16];
    DWORD smc_status;
    unsigned char region1[256];
    unsigned char dram[256];
    unsigned char rec[532];
    int r1_ok = 1;
    int dram_ok = 1;
    int i;

    memset(smc_req, 0, sizeof(smc_req));
    memset(smc_resp, 0, sizeof(smc_resp));
    smc_req[0] = 0x01;
    smc_status = HalSendSMCMessage(smc_req, smc_resp);

    memset(region1, 0, sizeof(region1));
    memset(dram, 0, sizeof(dram));

    __try {
        memcpy(region1, (const void *)0x010000000000ULL, sizeof(region1));
    } __except (1) {
        r1_ok = 0;
    }
    __try {
        memcpy(dram, (const void *)0x8000000000000000ULL, sizeof(dram));
    } __except (1) {
        dram_ok = 0;
    }

    memset(rec, 0, sizeof(rec));
    rec[0] = (BYTE)(smc_status >> 24);
    rec[1] = (BYTE)(smc_status >> 16);
    rec[2] = (BYTE)(smc_status >> 8);
    rec[3] = (BYTE)smc_status;
    memcpy(rec + 4, smc_resp, 16);
    rec[20] = (BYTE)r1_ok;
    rec[21] = (BYTE)dram_ok;
    memcpy(rec + 24, region1, 256);
    memcpy(rec + 280, dram, 256);

    write_named_file("capture_all.bin", rec, sizeof(rec));
}
