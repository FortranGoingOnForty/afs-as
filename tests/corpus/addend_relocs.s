	.text
	.globl	_caller
_caller:
	bl	_puts + 4
	adrp	x0, _data@PAGE + 0x24
	ldr	x0, [x0, _data@PAGEOFF + 0x24]
	ret
