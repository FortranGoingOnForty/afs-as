	.section	__TEXT,__text,regular,pure_instructions
	.build_version macos, 11, 0	sdk_version 15, 5
	.globl	_get_lane_u64                   ; -- Begin function get_lane_u64
	.p2align	2
_get_lane_u64:                          ; @get_lane_u64
	.cfi_startproc
; %bb.0:
	mov.d	x0, v0[1]
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_set_lane_u64                   ; -- Begin function set_lane_u64
	.p2align	2
_set_lane_u64:                          ; @set_lane_u64
	.cfi_startproc
; %bb.0:
	mov.d	v0[1], x0
	ret
	.cfi_endproc
                                        ; -- End function
.subsections_via_symbols
