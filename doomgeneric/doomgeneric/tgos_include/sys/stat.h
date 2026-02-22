/* Stub sys/stat.h */
#ifndef _SYS_STAT_H
#define _SYS_STAT_H
struct stat { long st_size; };
int stat(const char *path, struct stat *st);
int mkdir(const char *path, int mode);
#endif
