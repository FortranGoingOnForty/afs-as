.globl _main
.private_extern _helper
.weak_reference _puts
.weak_definition _entry

.build_version macos, 15, 0
.text
_entry:
_main:
    bl _puts
    bl _helper
_helper:
    ret
