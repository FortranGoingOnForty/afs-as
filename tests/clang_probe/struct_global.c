struct Pair {
    int a;
    int b;
};

struct Pair g_pair = {1, 2};

int sum_pair(void) { return g_pair.a + g_pair.b; }
