	.section	__TEXT,__text,regular,pure_instructions
	.build_version macos, 11, 0	sdk_version 15, 5
	.globl	_dup_f32                        ; -- Begin function dup_f32
	.p2align	2
_dup_f32:                               ; @dup_f32
	.cfi_startproc
; %bb.0:
	dup.4s	v0, v0[2]
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_dup_f64                        ; -- Begin function dup_f64
	.p2align	2
_dup_f64:                               ; @dup_f64
	.cfi_startproc
; %bb.0:
	dup.2d	v0, v0[1]
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_dup_s32                        ; -- Begin function dup_s32
	.p2align	2
_dup_s32:                               ; @dup_s32
	.cfi_startproc
; %bb.0:
	dup.4s	v0, v0[3]
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_dup_s16                        ; -- Begin function dup_s16
	.p2align	2
_dup_s16:                               ; @dup_s16
	.cfi_startproc
; %bb.0:
	dup.8h	v0, v0[5]
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_dup_s8                         ; -- Begin function dup_s8
	.p2align	2
_dup_s8:                                ; @dup_s8
	.cfi_startproc
; %bb.0:
	dup.16b	v0, v0[7]
	ret
	.cfi_endproc
                                        ; -- End function
.subsections_via_symbols
