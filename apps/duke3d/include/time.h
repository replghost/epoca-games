#ifndef _TIME_H
#define _TIME_H

/* time_t — calendar time as seconds since epoch (long on riscv32) */
typedef long time_t;

/* clock_t — processor time (ticks) */
typedef long clock_t;

/* CLOCKS_PER_SEC — our stub clock() always returns 0, so this is nominal */
#define CLOCKS_PER_SEC 1000000L

struct tm {
    int tm_sec;
    int tm_min;
    int tm_hour;
    int tm_mday;
    int tm_mon;
    int tm_year;
    int tm_wday;
    int tm_yday;
    int tm_isdst;
};

/* Time functions — implemented in libc_shim.c (stubs returning 0) */
time_t  time(time_t *t);
clock_t clock(void);
struct tm *localtime(const time_t *time);

#endif /* _TIME_H */
