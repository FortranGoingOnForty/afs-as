typedef float v4f __attribute__((vector_size(16)));

v4f add4(v4f a, v4f b) {
    return a + b;
}

void add4_store(float *out, const float *a, const float *b) {
    *(v4f *)out = *(const v4f *)a + *(const v4f *)b;
}
