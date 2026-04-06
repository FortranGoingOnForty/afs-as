double clampish(double a, double b) {
    double x = a * b + 3.5;
    return x < 10.0 ? x : x - 1.0;
}
