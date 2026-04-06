	.section	__TEXT,__text,regular,pure_instructions
	.build_version macos, 15, 0	sdk_version 15, 5
	.globl	_load_tls                       ; -- Begin function load_tls
	.p2align	2
_load_tls:                              ; @load_tls
	.cfi_startproc
; %bb.0:
	adrp	x0, _tls_counter@TLVPPAGE
	ldr	x0, [x0, _tls_counter@TLVPPAGEOFF]
	ldr	x8, [x0]
	blr	x8
	ldr	x0, [x0]
	ret
	.cfi_endproc
                                        ; -- End function
.subsections_via_symbols
