; Workbench note: Apple `as` encoding probe for the BIF/BIT/BSL select/blend
; family after the real clang blend probe showed `vbslq_*` lowering to BIF.
.text
bif.16b v0, v1, v2
bit.16b v3, v4, v5
bsl.16b v6, v7, v8
