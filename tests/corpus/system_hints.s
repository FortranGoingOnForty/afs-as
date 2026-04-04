.text
.globl _system_hints
_system_hints:
    yield
    wfe
    wfi
    sev
    sevl
    isb
    dmb ish
    dsb ishst
    ret
