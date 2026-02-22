/* Stub stdlib.h */
#ifndef _STDLIB_H
#define _STDLIB_H
#include <stddef.h>
void *malloc(size_t size);
void *calloc(size_t n, size_t sz);
void *realloc(void *p, size_t sz);
void free(void *p);
void exit(int code);
void abort(void);
int atexit(void (*fn)(void));
int atoi(const char *s);
long atol(const char *s);
double atof(const char *s);
long strtol(const char *s, char **end, int base);
unsigned long strtoul(const char *s, char **end, int base);
int abs(int x);
int rand(void);
void srand(unsigned int seed);
void qsort(void *base, size_t nmemb, size_t size, int (*compar)(const void*, const void*));
char *getenv(const char *name);
int system(const char *cmd);
#define EXIT_SUCCESS 0
#define EXIT_FAILURE 1
#define NULL ((void*)0)
#define RAND_MAX 0x7fff
#endif
