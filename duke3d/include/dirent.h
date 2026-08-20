#ifndef EPOCA_DIRENT_H
#define EPOCA_DIRENT_H

#include <stddef.h>

#define _D_EXACT_NAMLEN(entry) ((int)strlen((entry)->d_name))

typedef struct _DIR DIR;
struct dirent {
    char d_name[256];
    unsigned short d_namlen;
};

DIR *opendir(const char *path);
struct dirent *readdir(DIR *directory);
int closedir(DIR *directory);

#endif
