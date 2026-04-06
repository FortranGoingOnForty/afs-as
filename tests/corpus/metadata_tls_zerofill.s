.section __TEXT,__text,regular,pure_instructions
.build_version macos, 14, 1
.globl _metadata_tls_entry
.p2align 2
_metadata_tls_entry:
    ret

.zerofill __DATA,__thread_bss,_tls_counter$tlv$init,4,2

.section __DATA,__thread_vars,thread_local_variables
.globl _tls_counter
_tls_counter:
    .quad __tlv_bootstrap
    .quad 0
    .quad _tls_counter$tlv$init

.subsections_via_symbols
