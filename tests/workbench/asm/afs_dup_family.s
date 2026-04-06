; Workbench note: initial Apple `as` encoding probe for the common DUP family
; that established the first known-good encodings for `.16b`, `.8h`, `.4s`,
; and `.2d`.
.text
dup.16b v0, v0[7]
dup.8h v1, v2[5]
dup.4s v3, v4[2]
dup.2d v5, v6[1]
