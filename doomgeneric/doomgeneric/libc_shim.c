/*
 * libc_shim.c — Minimal C library for Doom on tg-ch8 OS
 *
 * Provides: malloc/free, printf/fprintf/sprintf/snprintf, fopen/fread/fwrite/fseek/ftell/fclose,
 *           memcpy/memset/memmove/memcmp, string ops, atoi, qsort, abs, rand, exit, etc.
 *
 * All I/O goes through RISC-V ecall (tg-ch8 syscalls).
 */

#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>

/* ═══════════════════════════════════════════════════════
 * Syscall layer
 * ═══════════════════════════════════════════════════════ */
static long _syscall1(long id, long a0) {
    register long _a0 __asm__("a0") = a0;
    register long _id __asm__("a7") = id;
    __asm__ volatile("ecall" : "+r"(_a0) : "r"(_id) : "memory");
    return _a0;
}
static long _syscall2(long id, long a0, long a1) {
    register long _a0 __asm__("a0") = a0;
    register long _a1 __asm__("a1") = a1;
    register long _id __asm__("a7") = id;
    __asm__ volatile("ecall" : "+r"(_a0) : "r"(_a1), "r"(_id) : "memory");
    return _a0;
}
static long _syscall3(long id, long a0, long a1, long a2) {
    register long _a0 __asm__("a0") = a0;
    register long _a1 __asm__("a1") = a1;
    register long _a2 __asm__("a2") = a2;
    register long _id __asm__("a7") = id;
    __asm__ volatile("ecall" : "+r"(_a0) : "r"(_a1), "r"(_a2), "r"(_id) : "memory");
    return _a0;
}

#define SYS_OPEN  56
#define SYS_CLOSE 57
#define SYS_READ  63
#define SYS_WRITE 64
#define SYS_EXIT  93
#define SYS_SCHED_YIELD 124
#define SYS_CLOCK_GETTIME 113

static long sys_write(long fd, const void *buf, long len) { return _syscall3(SYS_WRITE, fd, (long)buf, len); }
static long sys_read(long fd, void *buf, long len)        { return _syscall3(SYS_READ,  fd, (long)buf, len); }
static long sys_open(const char *path, long flags)        { return _syscall3(SYS_OPEN, (long)path, flags, (long)__builtin_strlen(path)); }
static long sys_close(long fd)                            { return _syscall1(SYS_CLOSE, fd); }

/* ═══════════════════════════════════════════════════════
 * Memory: bump allocator on static buffer
 * Doom needs ~8MB, we give it 10MB
 * ═══════════════════════════════════════════════════════ */
#define HEAP_SIZE (32 * 1024 * 1024)
static char _heap[HEAP_SIZE] __attribute__((aligned(16)));
static size_t _heap_ptr = 0;

struct alloc_hdr {
    size_t size;
    size_t dummy; // Pad to 16 bytes
};

void *malloc(size_t size) {
    size_t alloc_sz = (size + 15) & ~15UL;
    if (_heap_ptr + sizeof(struct alloc_hdr) + alloc_sz > HEAP_SIZE) return (void*)0;
    struct alloc_hdr *hdr = (struct alloc_hdr *)&_heap[_heap_ptr];
    hdr->size = alloc_sz;
    _heap_ptr += sizeof(struct alloc_hdr) + alloc_sz;
    return (void *)(hdr + 1);
}

void *calloc(size_t n, size_t sz) {
    size_t total = n * sz;
    void *p = malloc(total);
    if (p) __builtin_memset(p, 0, total);
    return p;
}

void *realloc(void *old, size_t sz) {
    if (!old) return malloc(sz);
    struct alloc_hdr *hdr = (struct alloc_hdr *)old - 1;
    size_t old_sz = hdr->size;
    size_t alloc_sz = (sz + 15) & ~15UL;
    
    /* Can we extend in place? */
    if ((char *)old + old_sz == &_heap[_heap_ptr]) {
        if (alloc_sz <= old_sz) return old;
        size_t diff = alloc_sz - old_sz;
        if (_heap_ptr + diff > HEAP_SIZE) return (void*)0;
        _heap_ptr += diff;
        hdr->size = alloc_sz;
        return old;
    }
    
    /* Bump and copy */
    void *p = malloc(sz);
    if (p) __builtin_memcpy(p, old, old_sz < sz ? old_sz : sz);
    return p;
}

void free(void *p) { (void)p; /* bump: no-op */ }

/* ═══════════════════════════════════════════════════════
 * String operations
 * ═══════════════════════════════════════════════════════ */
void *memcpy(void *dst, const void *src, size_t n) {
    unsigned char *d = dst; const unsigned char *s = src;
    while (n--) *d++ = *s++;
    return dst;
}
void *memmove(void *dst, const void *src, size_t n) {
    unsigned char *d = dst; const unsigned char *s = src;
    if (d < s) { while (n--) *d++ = *s++; }
    else { d += n; s += n; while (n--) *--d = *--s; }
    return dst;
}
void *memset(void *s, int c, size_t n) {
    unsigned char *p = s;
    while (n--) *p++ = (unsigned char)c;
    return s;
}
int memcmp(const void *a, const void *b, size_t n) {
    const unsigned char *p = a, *q = b;
    for (size_t i = 0; i < n; i++) { if (p[i] != q[i]) return p[i] - q[i]; }
    return 0;
}

size_t strlen(const char *s) { size_t n = 0; while (s[n]) n++; return n; }
char *strcpy(char *d, const char *s) { char *r = d; while ((*d++ = *s++)); return r; }
char *strncpy(char *d, const char *s, size_t n) {
    size_t i;
    for (i = 0; i < n && s[i]; i++) d[i] = s[i];
    for (; i < n; i++) d[i] = 0;
    return d;
}
char *strcat(char *d, const char *s) { strcpy(d + strlen(d), s); return d; }
char *strncat(char *d, const char *s, size_t n) {
    size_t dl = strlen(d); size_t i;
    for (i = 0; i < n && s[i]; i++) d[dl + i] = s[i];
    d[dl + i] = 0;
    return d;
}
int strcmp(const char *a, const char *b) {
    while (*a && *a == *b) { a++; b++; }
    return *(unsigned char*)a - *(unsigned char*)b;
}
int strncmp(const char *a, const char *b, size_t n) {
    for (size_t i = 0; i < n; i++) {
        if (a[i] != b[i] || !a[i]) return (unsigned char)a[i] - (unsigned char)b[i];
    }
    return 0;
}

static int _lower(int c) { return (c >= 'A' && c <= 'Z') ? c + 32 : c; }

int strcasecmp(const char *a, const char *b) {
    while (*a && _lower(*a) == _lower(*b)) { a++; b++; }
    return _lower(*(unsigned char*)a) - _lower(*(unsigned char*)b);
}
int strncasecmp(const char *a, const char *b, size_t n) {
    for (size_t i = 0; i < n; i++) {
        int la = _lower((unsigned char)a[i]), lb = _lower((unsigned char)b[i]);
        if (la != lb || !a[i]) return la - lb;
    }
    return 0;
}
char *strchr(const char *s, int c) { while (*s) { if (*s == (char)c) return (char*)s; s++; } return c ? 0 : (char*)s; }
char *strrchr(const char *s, int c) {
    const char *r = 0;
    while (*s) { if (*s == (char)c) r = s; s++; }
    return (char*)(c ? r : s);
}
char *strstr(const char *h, const char *n) {
    size_t nl = strlen(n);
    if (!nl) return (char*)h;
    while (*h) { if (!strncmp(h, n, nl)) return (char*)h; h++; }
    return 0;
}
char *strdup(const char *s) {
    size_t l = strlen(s) + 1;
    char *d = malloc(l);
    if (d) memcpy(d, s, l);
    return d;
}

/* ═══════════════════════════════════════════════════════
 * ctype
 * ═══════════════════════════════════════════════════════ */
int isspace(int c)  { return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' || c == '\v'; }
int isdigit(int c)  { return c >= '0' && c <= '9'; }
int isalnum(int c)  { return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'); }
int isprint(int c)  { return c >= 0x20 && c <= 0x7e; }
int isupper(int c)  { return c >= 'A' && c <= 'Z'; }
int islower(int c)  { return c >= 'a' && c <= 'z'; }
int toupper(int c)  { return islower(c) ? c - 32 : c; }
int tolower(int c)  { return isupper(c) ? c + 32 : c; }

/* ═══════════════════════════════════════════════════════
 * Number conversion
 * ═══════════════════════════════════════════════════════ */
int atoi(const char *s) {
    int n = 0, neg = 0;
    while (isspace(*s)) s++;
    if (*s == '-') { neg = 1; s++; } else if (*s == '+') s++;
    while (isdigit(*s)) { n = n * 10 + (*s - '0'); s++; }
    return neg ? -n : n;
}
long atol(const char *s) { return (long)atoi(s); }
long strtol(const char *s, char **end, int base) {
    (void)base; /* simplified: only base-10 */
    long n = atoi(s);
    if (end) { while (isspace(*s)) s++; if (*s == '-' || *s == '+') s++; while (isdigit(*s)) s++; *end = (char*)s; }
    return n;
}
unsigned long strtoul(const char *s, char **end, int base) { return (unsigned long)strtol(s, end, base); }

/* ═══════════════════════════════════════════════════════
 * printf family (minimal)
 * ═══════════════════════════════════════════════════════ */

/* Core formatter: writes into buf[0..size-1], returns total length (may exceed size) */
static int _vformat(char *buf, size_t size, const char *fmt, va_list ap) {
    size_t pos = 0;
    #define PUTC(c) do { if (pos < size) buf[pos] = (c); pos++; } while(0)

    while (*fmt) {
        if (*fmt != '%') { PUTC(*fmt++); continue; }
        fmt++; /* skip % */

        /* flags/width/precision (simplified) */
        int pad = 0, zero = 0, left = 0, have_prec = 0;
        long prec = 0;
        if (*fmt == '-') { left = 1; fmt++; }
        if (*fmt == '0') { zero = 1; fmt++; }
        while (isdigit(*fmt)) { pad = pad * 10 + (*fmt - '0'); fmt++; }
        if (*fmt == '.') { fmt++; have_prec = 1; while (isdigit(*fmt)) { prec = prec * 10 + (*fmt - '0'); fmt++; } }

        /* length modifier */
        int is_long = 0;
        if (*fmt == 'l') { is_long = 1; fmt++; if (*fmt == 'l') { is_long = 2; fmt++; } }
        else if (*fmt == 'z') { is_long = 1; fmt++; }

        char tmp[32];
        int tlen = 0;
        switch (*fmt) {
        case 'd': case 'i': {
            long v = is_long ? va_arg(ap, long) : (long)va_arg(ap, int);
            int neg = 0;
            unsigned long uv;
            if (v < 0) { neg = 1; uv = (unsigned long)(-v); } else uv = (unsigned long)v;
            if (uv == 0) tmp[tlen++] = '0';
            else while (uv) { tmp[tlen++] = '0' + (uv % 10); uv /= 10; }
            if (neg) tmp[tlen++] = '-';
            /* reverse */
            for (int i = 0; i < tlen/2; i++) { char c=tmp[i]; tmp[i]=tmp[tlen-1-i]; tmp[tlen-1-i]=c; }
            break;
        }
        case 'u': {
            unsigned long v = is_long ? va_arg(ap, unsigned long) : (unsigned long)va_arg(ap, unsigned int);
            if (v == 0) tmp[tlen++] = '0';
            else while (v) { tmp[tlen++] = '0' + (v % 10); v /= 10; }
            for (int i = 0; i < tlen/2; i++) { char c=tmp[i]; tmp[i]=tmp[tlen-1-i]; tmp[tlen-1-i]=c; }
            break;
        }
        case 'x': case 'X': {
            unsigned long v = is_long ? va_arg(ap, unsigned long) : (unsigned long)va_arg(ap, unsigned int);
            const char *hex = (*fmt == 'x') ? "0123456789abcdef" : "0123456789ABCDEF";
            if (v == 0) tmp[tlen++] = '0';
            else while (v) { tmp[tlen++] = hex[v & 0xf]; v >>= 4; }
            for (int i = 0; i < tlen/2; i++) { char c=tmp[i]; tmp[i]=tmp[tlen-1-i]; tmp[tlen-1-i]=c; }
            break;
        }
        case 'p': {
            unsigned long v = (unsigned long)va_arg(ap, void*);
            PUTC('0'); PUTC('x');
            const char *hex = "0123456789abcdef";
            if (v == 0) tmp[tlen++] = '0';
            else while (v) { tmp[tlen++] = hex[v & 0xf]; v >>= 4; }
            for (int i = 0; i < tlen/2; i++) { char c=tmp[i]; tmp[i]=tmp[tlen-1-i]; tmp[tlen-1-i]=c; }
            break;
        }
        case 's': {
            const char *s = va_arg(ap, const char*);
            if (!s) s = "(null)";
            int sl = (int)strlen(s);
            if (have_prec && prec < sl) sl = (int)prec;
            int padding = pad > sl ? pad - sl : 0;
            if (!left) for (int i = 0; i < padding; i++) PUTC(' ');
            for (int i = 0; i < sl; i++) PUTC(s[i]);
            if (left)  for (int i = 0; i < padding; i++) PUTC(' ');
            fmt++;
            continue;
        }
        case 'c': { char c = (char)va_arg(ap, int); PUTC(c); fmt++; continue; }
        case '%': PUTC('%'); fmt++; continue;
        default: PUTC('%'); PUTC(*fmt); fmt++; continue;
        }
        /* Emit tmp with padding */
        int padding = 0;
        if (have_prec) {
            padding = prec > tlen ? prec - tlen : 0;
            zero = 1; /* Precision on integers forces leading zeros */
        } else {
            padding = pad > tlen ? pad - tlen : 0;
        }
        
        char pch = (zero && !left) ? '0' : ' ';
        if (!left) for (int i = 0; i < padding; i++) PUTC(pch);
        for (int i = 0; i < tlen; i++) PUTC(tmp[i]);
        if (left) for (int i = 0; i < padding; i++) PUTC(' ');
        fmt++;
    }
    if (pos < size) buf[pos] = 0; else if (size > 0) buf[size-1] = 0;
    return (int)pos;
    #undef PUTC
}

int vsnprintf(char *buf, size_t size, const char *fmt, va_list ap) {
    return _vformat(buf, size, fmt, ap);
}
int snprintf(char *buf, size_t size, const char *fmt, ...) {
    va_list ap; va_start(ap, fmt); int r = vsnprintf(buf, size, fmt, ap); va_end(ap); return r;
}
int vsprintf(char *buf, const char *fmt, va_list ap) { return _vformat(buf, 4096, fmt, ap); }
int sprintf(char *buf, const char *fmt, ...) {
    va_list ap; va_start(ap, fmt); int r = vsprintf(buf, fmt, ap); va_end(ap); return r;
}
int vfprintf(void *stream, const char *fmt, va_list ap) {
    char buf[1024];
    int n = vsnprintf(buf, sizeof(buf), fmt, ap);
    if (n > 0) sys_write(1, buf, n < (int)sizeof(buf) ? n : (int)sizeof(buf)-1);
    return n;
}
int fprintf(void *stream, const char *fmt, ...) {
    va_list ap; va_start(ap, fmt); int r = vfprintf(stream, fmt, ap); va_end(ap); return r;
}
int printf(const char *fmt, ...) {
    va_list ap; va_start(ap, fmt); int r = vfprintf((void*)1, fmt, ap); va_end(ap); return r;
}
int puts(const char *s) { int n = (int)strlen(s); sys_write(1, s, n); sys_write(1, "\n", 1); return n+1; }
int putchar(int c) { char ch = (char)c; sys_write(1, &ch, 1); return c; }
int fputc(int c, void *f) { return putchar(c); }
int fputs(const char *s, void *f) { return (int)sys_write(1, s, (long)strlen(s)); }

/* ═══════════════════════════════════════════════════════
 * sscanf (very minimal: supports %d %s %x only)
 * ═══════════════════════════════════════════════════════ */
int sscanf(const char *str, const char *fmt, ...) {
    va_list ap; va_start(ap, fmt);
    int count = 0;
    while (*fmt && *str) {
        if (*fmt == '%') {
            fmt++;
            if (*fmt == 'd') {
                int *p = va_arg(ap, int*);
                *p = atoi(str);
                if (*str == '-' || *str == '+') str++;
                while (isdigit(*str)) str++;
                count++;
            } else if (*fmt == 'x') {
                int *p = va_arg(ap, int*);
                unsigned v = 0;
                while ((*str >= '0' && *str <= '9') || (*str >= 'a' && *str <= 'f') || (*str >= 'A' && *str <= 'F')) {
                    if (*str >= '0' && *str <= '9') v = v*16 + *str - '0';
                    else if (*str >= 'a' && *str <= 'f') v = v*16 + *str - 'a' + 10;
                    else v = v*16 + *str - 'A' + 10;
                    str++;
                }
                *p = (int)v;
                count++;
            } else if (*fmt == 's') {
                char *p = va_arg(ap, char*);
                while (*str && !isspace(*str)) *p++ = *str++;
                *p = 0;
                count++;
            }
            fmt++;
        } else {
            if (*fmt == *str) { fmt++; str++; }
            else break;
        }
    }
    va_end(ap);
    return count;
}

/* ═══════════════════════════════════════════════════════
 * File I/O (mapped to easy-fs via sys_open/read/write/close)
 *
 * Doom reads doom1.wad: fopen + fread + fseek + ftell
 * ═══════════════════════════════════════════════════════ */

/* We support up to 8 open files */
#define MAX_FILES 8

typedef struct {
    int fd;   /* kernel fd, or -1 if unused */
    long pos; /* cursor for fseek/ftell */
    long size;
    int eof;
    char filename[64];
    char *cache;
    long cache_size;
} MYFILE;

static MYFILE _files[MAX_FILES];

typedef MYFILE FILE;

/* Read the whole file size by reading until we get less than requested */
static long _get_file_size(int fd) {
    /* We'll just return a large number and let EOF detection work */
    return 0x7FFFFFFF;
}

FILE *fopen(const char *path, const char *mode) {
    /* Find the basename (strip directories) */
    const char *base = path;
    for (const char *p = path; *p; p++) { if (*p == '/') base = p + 1; }

    /* Translate mode to open flags */
    /* RDONLY=0, WRONLY=1, RDWR=2, CREATE=512, TRUNC=1024 */
    long flags = 0;
    if (__builtin_strchr(mode, '+')) flags = 2; /* RDWR */
    else if (__builtin_strchr(mode, 'w') || __builtin_strchr(mode, 'a')) flags = 1; /* WRONLY */
    else flags = 0; /* RDONLY */

    if (__builtin_strchr(mode, 'w')) flags |= 1024 | 512; /* TRUNC | CREATE */
    if (__builtin_strchr(mode, 'a')) flags |= 512; /* CREATE */

    long fd = sys_open(base, flags);
    if (fd < 0) return (FILE*)0;
    for (int i = 0; i < MAX_FILES; i++) {
        if (_files[i].fd < 0) {
            _files[i].fd = (int)fd;
            _files[i].pos = 0;
            _files[i].eof = 0;
            _files[i].cache = (void*)0;
            _files[i].cache_size = 0;
            for (int k = 0; k < 63 && base[k]; k++) {
                _files[i].filename[k] = base[k];
                _files[i].filename[k+1] = 0;
            }
            
            /* Cache .wad files to avoid slow seeking */
            if (__builtin_strstr(base, ".wad") || __builtin_strstr(base, ".WAD")) {
                char *buf = malloc(6 * 1024 * 1024); /* 6MB for doom1.wad */
                if (buf) {
                    long total = 0;
                    long r;
                    while ((r = sys_read(fd, buf + total, 65536)) > 0) {
                        total += r;
                    }
                    _files[i].cache = buf;
                    _files[i].cache_size = total;
                    _files[i].size = total;
                } else {
                    _files[i].size = _get_file_size((int)fd);
                }
            } else {
                _files[i].size = _get_file_size((int)fd);
            }
            return &_files[i];
        }
    }
    sys_close(fd);
    return (FILE*)0;
}

int fclose(FILE *f) {
    if (!f || f->fd < 0) return -1;
    sys_close(f->fd);
    /* We don't free cache here because malloc/free is a bump allocator, 
       but for hygiene we reset it. Actually, we should avoid multiple opens if possible. */
    f->fd = -1;
    f->cache = (void*)0;
    return 0;
}

size_t fread(void *buf, size_t size, size_t count, FILE *f) {
    if (!f || f->fd < 0) return 0;
    size_t total = size * count;
    if (f->cache) {
        if (f->pos >= f->cache_size) { f->eof = 1; return 0; }
        size_t available = f->cache_size - f->pos;
        if (total > available) total = available;
        __builtin_memcpy(buf, f->cache + f->pos, total);
        f->pos += total;
        return total / size;
    }
    long r = sys_read(f->fd, buf, (long)total);
    if (r <= 0) { f->eof = 1; return 0; }
    f->pos += r;
    return (size_t)r / size;
}

size_t fwrite(const void *buf, size_t size, size_t count, FILE *f) {
    if (!f || f->fd < 0) return 0;
    size_t total = size * count;
    long r = sys_write(f->fd, buf, (long)total);
    if (r <= 0) return 0;
    f->pos += r;
    return (size_t)r / size;
}

/* Simplified fseek: re-open file and read to position */
int fseek(FILE *f, long offset, int whence) {
    if (!f || f->fd < 0) return -1;
    long new_pos = f->pos;
    if (whence == 0 /* SEEK_SET */) new_pos = offset;
    else if (whence == 1 /* SEEK_CUR */) new_pos = f->pos + offset;
    else if (whence == 2 /* SEEK_END */) new_pos = f->size + offset;

    if (new_pos < 0) new_pos = 0;
    
    if (f->cache) {
        if (new_pos > f->cache_size) new_pos = f->cache_size;
        f->pos = new_pos;
        f->eof = 0;
        return 0;
    }

    if (new_pos < f->pos) {
        /* easy-fs doesn't have lseek, reopen to seek backward */
        sys_close(f->fd);
        long new_fd = sys_open(f->filename, 0);
        if (new_fd < 0) return -1;
        f->fd = (int)new_fd;
        f->pos = 0;
    }
    
    long skip = new_pos - f->pos;
    char tmp[512];
    while (skip > 0) {
        long chunk = skip > 512 ? 512 : skip;
        long r = sys_read(f->fd, tmp, chunk);
        if (r <= 0) break;
        skip -= r;
        f->pos += r;
    }
    f->eof = 0;
    return 0;
}

long ftell(FILE *f) { return f ? f->pos : -1; }
int feof(FILE *f) { return f ? f->eof : 1; }
char *fgets(char *s, int n, FILE *f) {
    if (!f || n <= 0) return 0;
    int i = 0;
    while (i < n - 1) {
        char c;
        if (sys_read(f->fd, &c, 1) <= 0) { f->eof = 1; break; }
        f->pos++;
        s[i++] = c;
        if (c == '\n') break;
    }
    if (i == 0) return 0;
    s[i] = 0;
    return s;
}

/* Doom redirect: stderr = stdout */
void *__stderrp = (void*)1;
void *__stdoutp = (void*)1;
void *stderr = (void*)1;
void *stdout = (void*)1;
void *stdin  = (void*)0;

/* ═══════════════════════════════════════════════════════
 * Misc standard library
 * ═══════════════════════════════════════════════════════ */
int abs(int x) { return x < 0 ? -x : x; }

static unsigned int _rand_seed = 12345;
int rand(void) { _rand_seed = _rand_seed * 1103515245 + 12345; return (_rand_seed >> 16) & 0x7fff; }
void srand(unsigned int seed) { _rand_seed = seed; }

/* qsort (simple insertion sort) */
void qsort(void *base, size_t nmemb, size_t size, int (*compar)(const void*, const void*)) {
    char *b = (char*)base;
    char tmp[256]; /* Doom elements are small */
    for (size_t i = 1; i < nmemb; i++) {
        memcpy(tmp, b + i * size, size);
        size_t j = i;
        while (j > 0 && compar(b + (j-1)*size, tmp) > 0) {
            memcpy(b + j*size, b + (j-1)*size, size);
            j--;
        }
        memcpy(b + j*size, tmp, size);
    }
}

void exit(int code) {
    _syscall1(SYS_EXIT, code);
    __builtin_unreachable();
}

void abort(void) { exit(-1); }

typedef void (*atexit_fn)(void);
static atexit_fn _atexit_fns[16];
static int _atexit_count = 0;
int atexit(atexit_fn fn) {
    if (_atexit_count < 16) { _atexit_fns[_atexit_count++] = fn; return 0; }
    return -1;
}

/* Stubbed functions that Doom calls but doesn't strictly need */
int system(const char *cmd) { (void)cmd; return -1; }
int fflush(FILE *f) { (void)f; return 0; }
int remove(const char *path) { (void)path; return 0; }
int rename(const char *old, const char *new_) { (void)old; (void)new_; return -1; }
char *getenv(const char *name) { (void)name; return (char*)0; }
int access(const char *path, int mode) { (void)path; (void)mode; return -1; }
int fileno(FILE *f) { return f ? f->fd : -1; }
int isatty(int fd) { (void)fd; return 0; }
int mkdir(const char *path, int mode) { (void)path; (void)mode; return -1; }
int usleep(unsigned long us) {
    (void)us;
    _syscall1(SYS_SCHED_YIELD, 0);
    return 0;
}

/* time — return milliseconds */
long time(long *t) {
    struct { long sec; long nsec; } ts;
    _syscall3(SYS_CLOCK_GETTIME, 1, (long)&ts, 0);
    long v = ts.sec;
    if (t) *t = v;
    return v;
}

struct timeval { long tv_sec; long tv_usec; };
struct timezone { int tz_minuteswest; int tz_dsttime; };
int gettimeofday(struct timeval *tv, struct timezone *tz) {
    struct { long sec; long nsec; } ts;
    _syscall3(SYS_CLOCK_GETTIME, 1, (long)&ts, 0);
    if (tv) { tv->tv_sec = ts.sec; tv->tv_usec = ts.nsec / 1000; }
    if (tz) { tz->tz_minuteswest = 0; tz->tz_dsttime = 0; }
    return 0;
}

/* Math/Float stubs */
double fabs(double x) { return x < 0 ? -x : x; }
double atof(const char *s) {
    double res = 0.0, fact = 1.0;
    int point = 0;
    if (*s == '-') { fact = -1.0; s++; }
    for (int loop = 0; loop < 2; loop++) {
        for (; *s >= '0' && *s <= '9'; s++) {
            if (point) fact /= 10.0;
            res = res * 10.0 + (*s - '0');
        }
        if (*s == '.') { point = 1; s++; } else break;
    }
    return res * fact;
}


/* stat structure (stub) */
struct stat { long st_size; };
int stat(const char *path, struct stat *st) {
    (void)path;
    if (st) st->st_size = 0;
    return -1;
}

/* ═══════════════════════════════════════════════════════
 * _start (entry point)
 * ═══════════════════════════════════════════════════════ */
extern int main(int argc, char **argv);

void _start(void) {
    /* Initialize file table */
    for (int i = 0; i < MAX_FILES; i++) _files[i].fd = -1;

    /* Doom expects i_wad_dir list; pass doom1.wad as arg */
    static char *argv[] = { "doom", "-iwad", "doom1.wad", 0 };
    int ret = main(3, argv);
    exit(ret);
}
