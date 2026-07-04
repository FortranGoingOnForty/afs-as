.text
.globl afs_modproc_l08_unalloc_val_base
.p2align 4
.type afs_modproc_l08_unalloc_val_base,@function
afs_modproc_l08_unalloc_val_base:
    pushq %rbp
    movq %rsp, %rbp
    movq %rdi, %rax
    movl $7, %ecx
    movl %ecx, %eax
    movq %rbp, %rsp
    popq %rbp
    ret
.size afs_modproc_l08_unalloc_val_base, .-afs_modproc_l08_unalloc_val_base

.text
.globl __prog_main
.p2align 4
.type __prog_main,@function
__prog_main:
    pushq %rbp
    movq %rsp, %rbp
    subq $432, %rsp
    movq %rbx, -400(%rbp)
    movq %r12, -408(%rbp)
    movq %r13, -416(%rbp)
    movq %r14, -424(%rbp)
    movq %r15, -432(%rbp)
    leaq -384(%rbp), %rbx
    xorl %eax, %eax
    movq $384, %rcx
    movq %rbx, %rdi
    movl %eax, %esi
    movq %rcx, %rdx
    xorl %eax, %eax
    call memset
    movq %rax, %rdx
    movq $16, %r12
    movq $1, %rsi
    movq %r12, %rdi
    imulq %rsi, %rdi
    leaq (%rbx,%rdi,1), %r8
    movl (%r8), %r13d
    testl %r13d, %r13d
    sete %r9b
    movzbl %r9b, %r14d
    testl %r14d, %r14d
    jne .L__prog_main_1
    jmp .L__prog_main_2
.L__prog_main_1:
    movq $32, %r15
    movq $1, %rax
    movq %r15, %rdx
    imulq %rax, %rdx
    movq %rbx, %rsi
    addq %rdx, %rsi
    movq (%rsi), %rdi
    movq %rdi, %r8
    movq %r8, %rcx
    jmp .L__prog_main_5
.L__prog_main_2:
    movl $15, %r9d
    cmpl %r9d, %r13d
    setl %r14b
    movzbl %r14b, %r15d
    testl %r15d, %r15d
    jne .L__prog_main_3
    jmp .L__prog_main_4
.L__prog_main_3:
    movl %r13d, %eax
    movq $24, %rdx
    movq %rax, %rsi
    imulq %rdx, %rsi
    leaq (%rdx,%rsi,1), %rdi
    movq $1, %r8
    movq %rdi, %r9
    imulq %r8, %r9
    movq %rbx, %r14
    addq %r9, %r14
    movq (%r14), %r15
    movq %r15, %rax
    movq %rax, %rcx
    jmp .L__prog_main_5
.L__prog_main_4:
    movq $0, %rdx
    movq %rdx, %rsi
    movq %rsi, %rdi
    movq %rdi, %rcx
    jmp .L__prog_main_5
.L__prog_main_5:
    movq %rcx, %r8
    testq %r8, %r8
    setne %r9b
    movzbl %r9b, %r14d
    testl %r14d, %r14d
    jne .L__prog_main_6
    jmp .L__prog_main_7
.L__prog_main_6:
    movq $1, %rax
    movq %r12, %r13
    imulq %rax, %r13
    leaq (%rcx,%r13,1), %rdx
    movq (%rdx), %r15
    movq %r15, %rsi
    testq %rsi, %rsi
    setne %dil
    movzbl %dil, %r8d
    testl %r8d, %r8d
    jne .L__prog_main_8
    jmp .L__prog_main_7
.L__prog_main_7:
    xorl %eax, %eax
    call afs_error_stop
    movq -400(%rbp), %rbx
    movq -408(%rbp), %r12
    movq -416(%rbp), %r13
    movq -424(%rbp), %r14
    movq -432(%rbp), %r15
    movq %rbp, %rsp
    popq %rbp
    ret
.L__prog_main_8:
    movq %rbx, %rdi
    xorl %eax, %eax
    movq %r15, %r11
    call *%r11
    movl %eax, %r9d
    movl $6, %r14d
    movl %r14d, %edi
    movl %r9d, %esi
    xorl %eax, %eax
    call afs_write_int
    movl %r14d, %edi
    xorl %eax, %eax
    call afs_write_newline
    leaq -388(%rbp), %rax
    movq %rbx, %rdi
    movq %rax, %rsi
    xorl %eax, %eax
    call afs_deallocate_array
    movq -400(%rbp), %rbx
    movq -408(%rbp), %r12
    movq -416(%rbp), %r13
    movq -424(%rbp), %r14
    movq -432(%rbp), %r15
    movq %rbp, %rsp
    popq %rbp
    ret
.size __prog_main, .-__prog_main

.data
.globl afs_vtable_l08_unalloc_base
.type afs_vtable_l08_unalloc_base, @object
.size afs_vtable_l08_unalloc_base, 24
.p2align 3
afs_vtable_l08_unalloc_base:
    .quad -5610194240673210171
    .quad 0
    .quad afs_modproc_l08_unalloc_val_base

.text
.globl main
.p2align 4
.type main,@function
main:
    pushq %rbp
    movq %rsp, %rbp
    callq afs_program_init
    callq __prog_main
    callq afs_program_finalize
    xorl %eax, %eax
    movq %rbp, %rsp
    popq %rbp
    ret
.size main, .-main
