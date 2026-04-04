extern int (*helper_slot)(int);

int call_helper_slot(int x) {
    return helper_slot(x + 2);
}
