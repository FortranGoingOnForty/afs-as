_Thread_local int tls_counter;

int bump_tls_counter(int delta) {
    tls_counter += delta;
    return tls_counter;
}
