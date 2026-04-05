typedef float v4f __attribute__((vector_size(16)));
typedef struct {
    v4f a;
    v4f b;
} pair;

pair make_pair(v4f x, v4f y) {
    pair p;
    p.a = x + y;
    p.b = x - y;
    return p;
}
