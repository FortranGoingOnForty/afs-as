	.section	__TEXT,__text,regular,pure_instructions
	.build_version macos, 15, 0	sdk_version 15, 5
	.globl	_addr                           ; -- Begin function addr
	.p2align	2
_addr:                                  ; @addr
	.cfi_startproc
; %bb.0:
Lloh0:
	adrp	x0, _value@PAGE
Lloh1:
	add	x0, x0, _value@PAGEOFF
	ret
	.loh AdrpAdd	Lloh0, Lloh1
	.cfi_endproc
                                        ; -- End function
	.section	__DATA,__data
	.p2align	2, 0x0                          ; @value
_value:
	.long	7                               ; 0x7

	.subsections_via_symbols
