#ifndef EPOCA_NES_COMPAT_H
#define EPOCA_NES_COMPAT_H

char *strtok(char *string, const char *delimiters);
void *memchr(const void *memory, int byte, unsigned long size);
double strtod(const char *string, char **end);
double atof(const char *string);

#endif
