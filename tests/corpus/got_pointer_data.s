	.build_version macos, 15, 0
	.data
	.globl	_ptr64
_ptr64:
	.quad	_puts@GOT

	.text
	.globl	_ptr32
_ptr32:
	.long	_puts@GOT - .
