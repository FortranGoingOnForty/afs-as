typedef float v4f __attribute__((vector_size(16)));

v4f sub4(v4f a, v4f b) { return a - b; }
v4f mul4(v4f a, v4f b) { return a * b; }
v4f div4(v4f a, v4f b) { return a / b; }
