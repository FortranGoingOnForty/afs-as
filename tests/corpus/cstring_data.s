.build_version macos, 15, 0
.global _main
.text
_main:
    ret

.section __TEXT,__cstring
greeting:
    .asciz "hello"
