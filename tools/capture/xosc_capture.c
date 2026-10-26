/* xosc_capture.c
 *
 * Xbox 360 homebrew skeleton (XDK 20353) that invokes the XOSC service and
 * writes the challenge/response next to the running .xex, with fallbacks to
 * the USB/HDD roots.  It also tries to send the same 56 bytes over TCP.
 *
 * Build: tools/build_xosc.bat
 *
 * The real XOSC entry point and buffer layout still have to be confirmed
 * against the HV.  The placeholders keep the shape of the exchange so the
 * capture path can be validated before the real call is wired in.
 */
#include <xtl.h>
#include <winsockx.h>
#include <stdio.h>
#include <string.h>

#define PC_IP   "192.168.1.100"
#define PC_PORT 9999

/* TODO: confirm with the real HV/kernel exports. */
#define XOSC_CHALLENGE_SIZE  20
#define XOSC_RESPONSE_SIZE   36
#define CAPTURE_NAME         "xosc_capture.bin"

/* Kernel ordinal 41. Prototype confirmed from the 17559 kernel dump:
 * r3 = 16-byte request (byte 0 = command), r4 = 16-byte response. */
extern DWORD HalSendSMCMessage(const BYTE *request, BYTE *response);

static void xosc_exchange(const unsigned char *challenge,
                          unsigned char *response) {
    memset(response, 0, XOSC_RESPONSE_SIZE);
    memcpy(response + 4, challenge, XOSC_CHALLENGE_SIZE);
    response[3] = 0xA5;
}

static int send_all(SOCKET s, const void *buf, int len) {
    const char *p = (const char *)buf;
    while (len > 0) {
        int n = send(s, p, len, 0);
        if (n <= 0) return -1;
        p += n;
        len -= n;
    }
    return 0;
}

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
    /* The Xbox 360 kernel needs the \??\ prefix for device paths.
     * ConsoleCapture uses "\??\game:\CAP..." successfully. */
    _snprintf(path, MAX_PATH, "\\??\\game:\\%s", name);
    if (write_one(path, buf, len)) return;
    _snprintf(path, MAX_PATH, "\\??\\mass0:\\%s", name);
    if (write_one(path, buf, len)) return;
    _snprintf(path, MAX_PATH, "\\??\\mass1:\\%s", name);
    if (write_one(path, buf, len)) return;
    _snprintf(path, MAX_PATH, "\\??\\hdd1:\\%s", name);
    if (write_one(path, buf, len)) return;
    /* Fallbacks without the prefix. */
    if (write_one(name, buf, len)) return;
    _snprintf(path, MAX_PATH, "Game:\\%s", name);
    if (write_one(path, buf, len)) return;
    _snprintf(path, MAX_PATH, "Usb0:\\%s", name);
    if (write_one(path, buf, len)) return;
    _snprintf(path, MAX_PATH, "Hdd1:\\%s", name);
    if (write_one(path, buf, len)) return;
}

static void write_capture_file(const void *buf, DWORD len) {
    write_named_file(CAPTURE_NAME, buf, len);
}

void __cdecl main(void) {
    unsigned char challenge[XOSC_CHALLENGE_SIZE];
    unsigned char response[XOSC_RESPONSE_SIZE];
    unsigned char packet[XOSC_CHALLENGE_SIZE + XOSC_RESPONSE_SIZE];
    BYTE smc_req[16];
    BYTE smc_resp[16];
    BYTE smc_record[20];
    DWORD smc_status;
    XNetStartupParams xnsp;
    WSADATA wsa;
    SOCKET s;
    struct sockaddr_in addr;

    memset(challenge, 0, sizeof(challenge));
    memset(response, 0, sizeof(response));

    /* Capture the SMC command 0x01 reply that the CD-stage trace lacks. */
    memset(smc_req, 0, sizeof(smc_req));
    memset(smc_resp, 0, sizeof(smc_resp));
    smc_req[0] = 0x01;
    smc_status = HalSendSMCMessage(smc_req, smc_resp);
    smc_record[0] = (BYTE)(smc_status >> 24);
    smc_record[1] = (BYTE)(smc_status >> 16);
    smc_record[2] = (BYTE)(smc_status >> 8);
    smc_record[3] = (BYTE)smc_status;
    memcpy(smc_record + 4, smc_resp, 16);
    write_named_file("smc_01.bin", smc_record, sizeof(smc_record));

    xosc_exchange(challenge, response);
    memcpy(packet, challenge, sizeof(challenge));
    memcpy(packet + sizeof(challenge), response, sizeof(response));
    /* Write first, before any network initialization can hang. */
    write_capture_file(packet, sizeof(packet));

    memset(&xnsp, 0, sizeof(xnsp));
    xnsp.cfgSizeOfStruct = sizeof(xnsp);
    xnsp.cfgFlags = XNET_STARTUP_BYPASS_SECURITY;
    if (XNetStartup(&xnsp) != 0) return;

    if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0) return;

    s = socket(AF_INET, SOCK_STREAM, 0);
    if (s != INVALID_SOCKET) {
        memset(&addr, 0, sizeof(addr));
        addr.sin_family = AF_INET;
        addr.sin_port = htons(PC_PORT);
        addr.sin_addr.s_addr = inet_addr(PC_IP);
        if (connect(s, (struct sockaddr *)&addr, sizeof(addr)) == 0) {
            send_all(s, packet, sizeof(packet));
        }
        closesocket(s);
    }
    WSACleanup();
    XNetCleanup();
}
