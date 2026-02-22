/* Stub unistd.h */
#ifndef _UNISTD_H
#define _UNISTD_H
#include <stddef.h>
int usleep(unsigned long us);
int isatty(int fd);
int access(const char *path, int mode);
#define R_OK 4
#define W_OK 2
#define X_OK 1
#define F_OK 0
#define STDIN_FILENO 0
#define STDOUT_FILENO 1
#define STDERR_FILENO 2
long read(int fd, void *buf, size_t n);
long write(int fd, const void *buf, size_t n);
int close(int fd);
#endif
