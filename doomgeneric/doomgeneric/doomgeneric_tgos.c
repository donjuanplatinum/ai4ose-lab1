/*
 * doomgeneric_tgos.c — Platform backend for tg-ch8 OS
 *
 * Implements the 6 required doomgeneric functions:
 *   DG_Init, DG_DrawFrame, DG_SleepMs, DG_GetTicksMs, DG_GetKey, DG_SetWindowTitle
 *
 * Uses:
 *   /dev/input for VirtIO-Input KEY_STATES
 *   /dev/gpu   for VirtIO-GPU framebuffer
 *   clock_gettime for timing
 */

#include "doomkeys.h"
#include "m_argv.h"
#include "doomgeneric.h"

/* ── Syscall wrappers (RISC-V ecall) ── */
static long syscall1(long id, long a0) {
    register long _a0 __asm__("a0") = a0;
    register long _id __asm__("a7") = id;
    __asm__ volatile("ecall" : "+r"(_a0) : "r"(_id) : "memory");
    return _a0;
}
static long syscall3(long id, long a0, long a1, long a2) {
    register long _a0 __asm__("a0") = a0;
    register long _a1 __asm__("a1") = a1;
    register long _a2 __asm__("a2") = a2;
    register long _id __asm__("a7") = id;
    __asm__ volatile("ecall" : "+r"(_a0) : "r"(_a1), "r"(_a2), "r"(_id) : "memory");
    return _a0;
}

#define SYS_OPEN  56
#define SYS_READ  63
#define SYS_WRITE 64
#define SYS_EXIT  93
#define SYS_SCHED_YIELD 124
#define SYS_CLOCK_GETTIME 113

static long sys_open(const char *path, long flags) { return syscall3(SYS_OPEN, (long)path, flags, 0); }
static long sys_read(long fd, void *buf, long len) { return syscall3(SYS_READ, fd, (long)buf, len); }
static long sys_write(long fd, const void *buf, long len) { return syscall3(SYS_WRITE, fd, (long)buf, len); }

struct timespec { long tv_sec; long tv_nsec; };

static long sys_clock_gettime(struct timespec *tp) {
    return syscall3(SYS_CLOCK_GETTIME, 1 /* CLOCK_MONOTONIC */, (long)tp, 0);
}

static void sys_yield(void) { syscall1(SYS_SCHED_YIELD, 0); }

/* ── Key queue ── */
#define KEYQUEUE_SIZE 32
static unsigned short s_KeyQueue[KEYQUEUE_SIZE];
static unsigned int s_KeyQueueWrite = 0;
static unsigned int s_KeyQueueRead  = 0;

static int input_fd = -1;
static int gpu_fd = -1;

/* Convert Linux evdev scancode → Doom key */
static unsigned char scancodeToDoom(unsigned char sc) {
    switch (sc) {
    case 28:  return KEY_ENTER;
    case 1:   return KEY_ESCAPE;
    case 105: return KEY_LEFTARROW;
    case 106: return KEY_RIGHTARROW;
    case 103: return KEY_UPARROW;
    case 108: return KEY_DOWNARROW;
    case 29:  return KEY_FIRE;       /* Left Ctrl */
    case 57:  return KEY_USE;        /* Space */
    case 42: case 54: return KEY_RSHIFT;
    case 56:  return KEY_RALT;       /* Alt = strafe */
    case 15:  return KEY_TAB;
    case 59:  return KEY_F1;
    case 60:  return KEY_F2;
    case 61:  return KEY_F3;
    case 62:  return KEY_F4;
    case 63:  return KEY_F5;
    case 64:  return KEY_F6;
    case 65:  return KEY_F7;
    case 66:  return KEY_F8;
    case 67:  return KEY_F9;
    case 68:  return KEY_F10;
    case 87:  return KEY_F11;
    case 88:  return KEY_F12;
    case 14:  return KEY_BACKSPACE;
    case 119: return KEY_PAUSE;
    case 12:  return KEY_MINUS;
    case 13:  return KEY_EQUALS;
    case 21:  return 'y';
    case 49:  return 'n';
    default:  return 0;
    }
}

static unsigned char prev_keys[256];

static void pollKeys(void) {
    if (input_fd < 0) return;
    unsigned char keys[256];
    sys_read(input_fd, keys, 256);

    for (int i = 0; i < 256; i++) {
        if (keys[i] != prev_keys[i]) {
            unsigned char dk = scancodeToDoom((unsigned char)i);
            if (dk != 0) {
                int pressed = keys[i] ? 1 : 0;
                s_KeyQueue[s_KeyQueueWrite] = (unsigned short)((pressed << 8) | dk);
                s_KeyQueueWrite = (s_KeyQueueWrite + 1) % KEYQUEUE_SIZE;
            }
            prev_keys[i] = keys[i];
        }
    }
}

/* ── Platform interface ── */

void DG_Init(void) {
    for (int i = 0; i < 256; i++) prev_keys[i] = 0;
    input_fd = sys_open("/dev/input", 0);
    gpu_fd = sys_open("/dev/gpu", 1); /* O_WRONLY */
}

void DG_DrawFrame(void) {
    /* Blit Doom's 32-bit RGBA buffer to the VirtIO-GPU framebuffer */
    if (gpu_fd >= 0) {
        sys_write(gpu_fd, DG_ScreenBuffer, DOOMGENERIC_RESX * DOOMGENERIC_RESY * 4);
    }
    pollKeys();
}

void DG_SleepMs(uint32_t ms) {
    struct timespec now;
    sys_clock_gettime(&now);
    long target_ns = now.tv_nsec + (long)ms * 1000000L;
    long target_sec = now.tv_sec + target_ns / 1000000000L;
    target_ns %= 1000000000L;

    for (;;) {
        sys_clock_gettime(&now);
        if (now.tv_sec > target_sec ||
            (now.tv_sec == target_sec && now.tv_nsec >= target_ns))
            break;
        sys_yield();
    }
}

uint32_t DG_GetTicksMs(void) {
    struct timespec tp;
    sys_clock_gettime(&tp);
    return (uint32_t)(tp.tv_sec * 1000 + tp.tv_nsec / 1000000);
}

int DG_GetKey(int* pressed, unsigned char* doomKey) {
    if (s_KeyQueueRead == s_KeyQueueWrite) return 0;
    unsigned short kd = s_KeyQueue[s_KeyQueueRead];
    s_KeyQueueRead = (s_KeyQueueRead + 1) % KEYQUEUE_SIZE;
    *pressed = kd >> 8;
    *doomKey = kd & 0xFF;
    return 1;
}

void DG_SetWindowTitle(const char *title) {
    (void)title;
}

/* ── Entry point ── */
int main(int argc, char **argv) {
    doomgeneric_Create(argc, argv);
    for (;;) {
        doomgeneric_Tick();
    }
    return 0;
}
