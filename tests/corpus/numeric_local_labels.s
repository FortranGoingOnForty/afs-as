.text
.globl _numeric_locals
_numeric_locals:
    mov x0, #0
1:
    add x0, x0, #1
    cmp x0, #2
    b.lt 1b
    cbz x1, 2f
    b .Ldone
2:
    mov x0, #7
.Ldone:
    ret
