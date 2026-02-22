/* Stub stdio.h for freestanding Doom build */
#ifndef _STDIO_H
#define _STDIO_H
#include <stddef.h>
#include <stdarg.h>

typedef struct { int fd; long pos; long size; int eof; } FILE;

extern FILE *stdin, *stdout, *stderr;
#define STDIN_FILENO 0
#define STDOUT_FILENO 1
#define STDERR_FILENO 2
#define EOF (-1)
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#define BUFSIZ 512

int printf(const char *fmt, ...);
int fprintf(void *stream, const char *fmt, ...);
int sprintf(char *buf, const char *fmt, ...);
int snprintf(char *buf, size_t size, const char *fmt, ...);
int vsnprintf(char *buf, size_t size, const char *fmt, va_list ap);
int vsprintf(char *buf, const char *fmt, va_list ap);
int vfprintf(void *stream, const char *fmt, va_list ap);
int sscanf(const char *str, const char *fmt, ...);
int puts(const char *s);
int putchar(int c);
int fputc(int c, void *f);
int fputs(const char *s, void *f);

FILE *fopen(const char *path, const char *mode);
int fclose(FILE *f);
size_t fread(void *buf, size_t size, size_t count, FILE *f);
size_t fwrite(const void *buf, size_t size, size_t count, FILE *f);
int fseek(FILE *f, long offset, int whence);
long ftell(FILE *f);
int feof(FILE *f);
char *fgets(char *s, int n, FILE *f);
int fflush(FILE *f);
int fileno(FILE *f);
int remove(const char *path);
int rename(const char *old, const char *new_);

#endif
