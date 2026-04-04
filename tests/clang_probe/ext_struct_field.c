struct Pair {
    int a;
    int b;
};

extern struct Pair ext_pair;

int read_ext_pair_b(void) {
    return ext_pair.b;
}
