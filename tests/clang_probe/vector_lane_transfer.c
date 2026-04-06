typedef float v4f __attribute__((vector_size(16)));
typedef double v2d __attribute__((vector_size(16)));

float lane2f(v4f x) { return x[2]; }

v4f set_lane0f(v4f x, float y) {
    x[0] = y;
    return x;
}

double lane1d(v2d x) { return x[1]; }

v2d set_lane1d(v2d x, double y) {
    x[1] = y;
    return x;
}
