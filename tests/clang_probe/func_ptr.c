extern int helper(int);

int call_helper_ptr(int x) {
    int (*fn)(int) = helper;
    return fn(x + 1);
}
