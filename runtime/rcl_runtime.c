#include <stdio.h>

void rcl_print(const char *s) {
    fputs(s, stdout);
    fflush(stdout);
}

void rcl_println(const char *s) {
    fputs(s, stdout);
    fputc('\n', stdout);
    fflush(stdout);
}
