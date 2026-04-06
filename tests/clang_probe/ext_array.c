extern int ext_array[4];

int read_ext_array_slot(int i) {
    return ext_array[i & 3];
}
