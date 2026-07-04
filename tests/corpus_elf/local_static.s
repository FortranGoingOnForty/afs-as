.text
.globl __prog_audit4_med5_nan_literal_init
.p2align 4
.type __prog_audit4_med5_nan_literal_init,@function
__prog_audit4_med5_nan_literal_init:
    pushq %rbp
    movq %rsp, %rbp
    subq $16, %rsp
    movq %rbx, -8(%rbp)
    leaq afs_save_audit4_med5_nan_literal_init_x(%rip), %rax
    movl $6, %ebx
    movss (%rax), %xmm1
    movl %ebx, %edi
    movss %xmm1, %xmm0
    movb $1, %al
    call afs_write_real
    movl %ebx, %edi
    xorl %eax, %eax
    call afs_write_newline
    movq -8(%rbp), %rbx
    movq %rbp, %rsp
    popq %rbp
    ret
.size __prog_audit4_med5_nan_literal_init, .-__prog_audit4_med5_nan_literal_init

.data
.local afs_save_audit4_med5_nan_literal_init_x
.type afs_save_audit4_med5_nan_literal_init_x, @object
.size afs_save_audit4_med5_nan_literal_init_x, 4
.p2align 2
afs_save_audit4_med5_nan_literal_init_x:
    .long 0xffc00000

.text
.globl main
.p2align 4
.type main,@function
main:
    pushq %rbp
    movq %rsp, %rbp
    callq afs_program_init
    callq __prog_audit4_med5_nan_literal_init
    callq afs_program_finalize
    xorl %eax, %eax
    movq %rbp, %rsp
    popq %rbp
    ret
.size main, .-main
