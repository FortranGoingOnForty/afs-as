	.section	__TEXT,__text,regular,pure_instructions
	.build_version macos, 11, 0	sdk_version 15, 5
	.globl	_blend_bytes                    ; -- Begin function blend_bytes
	.p2align	2
_blend_bytes:                           ; @blend_bytes
	.cfi_startproc
; %bb.0:
	bif.16b	v0, v1, v2
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_blend_u32                      ; -- Begin function blend_u32
	.p2align	2
_blend_u32:                             ; @blend_u32
	.cfi_startproc
; %bb.0:
	bif.16b	v0, v1, v2
	ret
	.cfi_endproc
                                        ; -- End function
.subsections_via_symbols
