extern int puts(const char *);

int call_puts(void) {
    return puts("hello from probe");
}
