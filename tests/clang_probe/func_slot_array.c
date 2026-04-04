extern int (*helper_slots[2])(int);

int call_helper_slot1(int x) {
    return helper_slots[1](x + 3);
}
