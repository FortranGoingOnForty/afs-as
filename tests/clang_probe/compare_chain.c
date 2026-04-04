int both_small(int a, int b) {
    if (a < 10 && b < 20) {
        if (a != 3 && b != 7) {
            return 2;
        }
        return 1;
    }
    return 0;
}
