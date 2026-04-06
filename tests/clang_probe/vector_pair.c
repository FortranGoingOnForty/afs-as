typedef float v4f __attribute__((vector_size(16)));

void use_pair(const v4f *in, v4f *out) {
    v4f a = in[0];
    v4f b = in[1];
    out[0] = a + b;
    out[1] = a - b;
}
