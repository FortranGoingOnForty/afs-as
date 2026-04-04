int copy_until_zero(char *dst, const char *src) {
    int i = 0;
    for (;;) {
        char c = src[i];
        dst[i] = c;
        if (!c) {
            return i;
        }
        i++;
    }
}
