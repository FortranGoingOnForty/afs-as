_Thread_local int tls_value;

int *tls_value_addr(void) {
    return &tls_value;
}
