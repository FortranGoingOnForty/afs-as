.text
.build_version macos, 11, 0 sdk_version 15, 5
.globl _entry
.private_extern _hidden
.weak_definition _weak_def
.weak_reference _puts
.p2align 2
_entry:
    bl _puts
    ret
_hidden:
    ret
_weak_def:
    ret

.subsections_via_symbols
