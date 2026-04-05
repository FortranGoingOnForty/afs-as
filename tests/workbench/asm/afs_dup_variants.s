; Workbench note: variant matrix used to derive the lane-index and size bit
; placement rule for DUP encodings, rather than treating each size as a
; separate hardcoded one-off.
.text
dup.16b v0, v1[0]
dup.16b v0, v1[7]
dup.8h v0, v1[0]
dup.8h v0, v1[7]
dup.4s v0, v1[0]
dup.4s v0, v1[3]
dup.2d v0, v1[0]
dup.2d v0, v1[1]
