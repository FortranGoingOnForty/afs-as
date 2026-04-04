unsigned bit_mix(unsigned x) {
    return ((x >> 3) & 31u) | ((x & 7u) << 5);
}
