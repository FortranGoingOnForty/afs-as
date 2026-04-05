; Workbench note: captured Apple clang `-S` output for half extraction and
; lane transfer/combine operations; this was kept to document why `ext.16b`,
; `mov.d`, and `mov.s` were the right next coverage points.
	.section	__TEXT,__text,regular,pure_instructions
	.build_version macos, 11, 0	sdk_version 15, 5
	.globl	_low_half                       ; -- Begin function low_half
	.p2align	2
_low_half:                              ; @low_half
	.cfi_startproc
; %bb.0:
                                        ; kill: def $d0 killed $d0 killed $q0
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_high_half                      ; -- Begin function high_half
	.p2align	2
_high_half:                             ; @high_half
	.cfi_startproc
; %bb.0:
	ext.16b	v0, v0, v0, #8
                                        ; kill: def $d0 killed $d0 killed $q0
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_combine_halves                 ; -- Begin function combine_halves
	.p2align	2
_combine_halves:                        ; @combine_halves
	.cfi_startproc
; %bb.0:
                                        ; kill: def $d1 killed $d1 def $q1
                                        ; kill: def $d0 killed $d0 def $q0
	mov.d	v0[1], v1[0]
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_set_lane                       ; -- Begin function set_lane
	.p2align	2
_set_lane:                              ; @set_lane
	.cfi_startproc
; %bb.0:
                                        ; kill: def $s1 killed $s1 def $q1
	mov.s	v0[2], v1[0]
	ret
	.cfi_endproc
                                        ; -- End function
.subsections_via_symbols
