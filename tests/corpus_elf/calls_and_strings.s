.text
.globl __prog_error_stop_status
.p2align 4
.type __prog_error_stop_status,@function
__prog_error_stop_status:
    pushq %rbp
    movq %rsp, %rbp
    xorl %eax, %eax
    call afs_error_stop
    movq %rbp, %rsp
    popq %rbp
    ret
.size __prog_error_stop_status, .-__prog_error_stop_status

.text
.globl main
.p2align 4
.type main,@function
main:
    pushq %rbp
    movq %rsp, %rbp
    callq afs_program_init
    callq __prog_error_stop_status
    callq afs_program_finalize
    xorl %eax, %eax
    movq %rbp, %rsp
    popq %rbp
    ret
.size main, .-main
