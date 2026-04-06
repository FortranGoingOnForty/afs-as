	.section	__TEXT,__text,regular,pure_instructions
	.build_version macos, 15, 0	sdk_version 15, 5
	.globl	_load_ext                        ; -- Begin function load_ext
	.p2align	2
_load_ext:                               ; @load_ext
	.cfi_startproc
; %bb.0:
	adrp	x8, _ext_global@GOTPAGE
	ldr	x8, [x8, _ext_global@GOTPAGEOFF]
	ldr	x0, [x8]
	ret
	.cfi_endproc
                                        ; -- End function
.subsections_via_symbols
