	.section	__TEXT,__text,regular,pure_instructions
	.build_version macos, 11, 0	sdk_version 15, 5
	.globl	_get_lane_u32                   ; -- Begin function get_lane_u32
	.p2align	2
_get_lane_u32:                          ; @get_lane_u32
	.cfi_startproc
; %bb.0:
	mov.s	w0, v0[2]
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_get_lane_u16                   ; -- Begin function get_lane_u16
	.p2align	2
_get_lane_u16:                          ; @get_lane_u16
	.cfi_startproc
; %bb.0:
	umov.h	w0, v0[5]
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_get_lane_u8                    ; -- Begin function get_lane_u8
	.p2align	2
_get_lane_u8:                           ; @get_lane_u8
	.cfi_startproc
; %bb.0:
	umov.b	w0, v0[7]
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_set_lane_u32                   ; -- Begin function set_lane_u32
	.p2align	2
_set_lane_u32:                          ; @set_lane_u32
	.cfi_startproc
; %bb.0:
	mov.s	v0[1], w0
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_set_lane_u16                   ; -- Begin function set_lane_u16
	.p2align	2
_set_lane_u16:                          ; @set_lane_u16
	.cfi_startproc
; %bb.0:
	mov.h	v0[5], w0
	ret
	.cfi_endproc
                                        ; -- End function
	.globl	_set_lane_u8                    ; -- Begin function set_lane_u8
	.p2align	2
_set_lane_u8:                           ; @set_lane_u8
	.cfi_startproc
; %bb.0:
	mov.b	v0[7], w0
	ret
	.cfi_endproc
                                        ; -- End function
.subsections_via_symbols
