int frame_heavy(int a, int b, int c, int d) {
    volatile int slots[8] = {a, b, c, d, 1, 2, 3, 4};
    return slots[0] + slots[7];
}
