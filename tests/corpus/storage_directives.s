.text
ret
.zerofill __DATA,__bss,_scratch,16,4
ret

.data
.short 0x1234
.fill 2, 2, 0x3344
.zero 3

.comm _common,24,3
