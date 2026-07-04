.text
.globl __prog_test_do_loop_vectorize_dot
.p2align 4
.type __prog_test_do_loop_vectorize_dot,@function
__prog_test_do_loop_vectorize_dot:
    pushq %rbp
    movq %rsp, %rbp
    subq $320, %rsp
    movq %rbx, -280(%rbp)
    movq %r12, -288(%rbp)
    movq %r13, -296(%rbp)
    movq %r14, -304(%rbp)
    movq %r15, -312(%rbp)
    leaq -128(%rbp), %rax
    leaq -256(%rbp), %rcx
    movl $1, %edx
    movl $32, %esi
    movl $0, %edi
    movq $1, %r8
    movl %edx, %ebx
    movl %ebx, %r9d
    jmp .L__prog_test_do_loop_vectorize_dot_1
.L__prog_test_do_loop_vectorize_dot_1:
    cmpl %esi, %r9d
    setle %r12b
    movzbl %r12b, %r13d
    testl %r13d, %r13d
    jne .L__prog_test_do_loop_vectorize_dot_2
    jmp .L__prog_test_do_loop_vectorize_dot_3
.L__prog_test_do_loop_vectorize_dot_2:
    movslq %r9d, %r14
    movq %r14, %r15
    subq %r8, %r15
    movq $4, %rbx
    movq %r15, %r12
    imulq %rbx, %r12
    leaq (%rax,%r12,1), %r13
    movl %r9d, (%r13)
    movq $4, %r14
    movq %r15, %rbx
    imulq %r14, %rbx
    leaq (%rcx,%rbx,1), %r12
    movl %r9d, (%r12)
    movl %r9d, %r13d
    addl %edx, %r13d
    movl %r13d, %r15d
    movl %r15d, %r9d
    jmp .L__prog_test_do_loop_vectorize_dot_1
.L__prog_test_do_loop_vectorize_dot_3:
    movd %edi, %xmm0
    pshufd $0, %xmm0, %xmm1
    movl $4, %r14d
    movl %edx, %r12d
    movaps %xmm1, %xmm2
    movl %r12d, %ebx
    movaps %xmm2, %xmm14
    movups %xmm14, -272(%rbp)
    jmp .L__prog_test_do_loop_vectorize_dot_4
.L__prog_test_do_loop_vectorize_dot_4:
    cmpl %esi, %ebx
    setle %r13b
    movzbl %r13b, %r15d
    testl %r15d, %r15d
    jne .L__prog_test_do_loop_vectorize_dot_5
    jmp .L__prog_test_do_loop_vectorize_dot_6
.L__prog_test_do_loop_vectorize_dot_5:
    movslq %ebx, %r9
    movq %r9, %r12
    subq %r8, %r12
    movq $4, %rdx
    movq %r12, %rdi
    imulq %rdx, %rdi
    leaq (%rax,%rdi,1), %r13
    movups (%r13), %xmm3
    movq $4, %r15
    movq %r12, %r9
    imulq %r15, %r9
    leaq (%rcx,%r9,1), %rdx
    movups (%rdx), %xmm4
    pshufd $245, %xmm3, %xmm5
    pshufd $245, %xmm4, %xmm6
    movaps %xmm3, %xmm7
    pmuludq %xmm4, %xmm7
    movaps %xmm5, %xmm8
    pmuludq %xmm6, %xmm8
    movaps %xmm7, %xmm9
    shufps $136, %xmm8, %xmm9
    pshufd $216, %xmm9, %xmm10
    movups -272(%rbp), %xmm14
    movaps %xmm14, %xmm11
    paddd %xmm10, %xmm11
    movl %ebx, %edi
    addl %r14d, %edi
    movl %edi, %r13d
    movaps %xmm11, %xmm12
    movl %r13d, %ebx
    movaps %xmm12, %xmm14
    movups %xmm14, -272(%rbp)
    jmp .L__prog_test_do_loop_vectorize_dot_4
.L__prog_test_do_loop_vectorize_dot_6:
    movups -272(%rbp), %xmm14
    pshufd $78, %xmm14, %xmm13
    movups -272(%rbp), %xmm14
    movaps %xmm14, %xmm0
    paddd %xmm13, %xmm0
    pshufd $229, %xmm0, %xmm1
    movaps %xmm0, %xmm2
    paddd %xmm1, %xmm2
    movd %xmm2, %r12d
    movl $6, %r15d
    movl %r15d, %edi
    movl %r12d, %esi
    xorl %eax, %eax
    call afs_write_int
    movl %r15d, %edi
    xorl %eax, %eax
    call afs_write_newline
    movq -280(%rbp), %rbx
    movq -288(%rbp), %r12
    movq -296(%rbp), %r13
    movq -304(%rbp), %r14
    movq -312(%rbp), %r15
    movq %rbp, %rsp
    popq %rbp
    ret
.size __prog_test_do_loop_vectorize_dot, .-__prog_test_do_loop_vectorize_dot

.text
.globl main
.p2align 4
.type main,@function
main:
    pushq %rbp
    movq %rsp, %rbp
    callq afs_program_init
    callq __prog_test_do_loop_vectorize_dot
    callq afs_program_finalize
    xorl %eax, %eax
    movq %rbp, %rsp
    popq %rbp
    ret
.size main, .-main
