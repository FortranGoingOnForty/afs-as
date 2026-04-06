	.section	__TEXT,__text,regular,pure_instructions
	.build_version macos, 15, 0	sdk_version 15, 5
	.globl	_add_and_load                   ; -- Begin function add_and_load
	.p2align	2
_add_and_load:                          ; @add_and_load
	.cfi_startproc
; %bb.0:
	fadd	d0, d0, d1
	ldr	d1, [x0]
	fadd	d0, d0, d1
	ret
	.cfi_endproc
                                        ; -- End function
	.subsections_via_symbols
