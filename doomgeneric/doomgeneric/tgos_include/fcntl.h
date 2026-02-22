/* Stub fcntl.h */
#ifndef _FCNTL_H
#define _FCNTL_H
#define O_RDONLY 0
#define O_WRONLY 1
#define O_RDWR   2
#define O_CREAT  0x40
int open(const char *path, int flags, ...);
#endif
