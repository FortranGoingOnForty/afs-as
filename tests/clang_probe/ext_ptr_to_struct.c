struct Pair {
    int a;
    int b;
};

extern struct Pair *ext_pair_ptr;

int read_ext_pair_ptr_b(void) {
    return ext_pair_ptr->b;
}
