.text
.globl __prog_audit4_c4_host_association
.p2align 4
.type __prog_audit4_c4_host_association,@function
__prog_audit4_c4_host_association:
    pushq %rbp
    movq %rsp, %rbp
    subq $16, %rsp
    movq %rbx, -8(%rbp)
    leaq afs_mod_audit4_c4_mod_v(%rip), %rax
    movl $6, %ebx
    movl (%rax), %ecx
    movl %ebx, %edi
    movl %ecx, %esi
    xorl %eax, %eax
    call afs_write_int
    movl %ebx, %edi
    xorl %eax, %eax
    call afs_write_newline
    movq -8(%rbp), %rbx
    movq %rbp, %rsp
    popq %rbp
    ret
.size __prog_audit4_c4_host_association, .-__prog_audit4_c4_host_association

.data
.globl afs_mod_audit4_c4_mod_v
.type afs_mod_audit4_c4_mod_v, @object
.size afs_mod_audit4_c4_mod_v, 4
.p2align 2
afs_mod_audit4_c4_mod_v:
    .long 42

.text
.globl main
.p2align 4
.type main,@function
main:
    pushq %rbp
    movq %rsp, %rbp
    callq afs_program_init
    callq __prog_audit4_c4_host_association
    callq afs_program_finalize
    xorl %eax, %eax
    movq %rbp, %rsp
    popq %rbp
    ret
.size main, .-main
