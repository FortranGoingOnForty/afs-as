_Thread_local int tls_value;

static int add_two(int *ptr) {
    *ptr += 2;
    return *ptr;
}

int bump_tls_via_ptr(void) {
    return add_two(&tls_value);
}
