_Thread_local int tls_value = 5;

int read_tls_plus_one(void) {
    return tls_value + 1;
}
