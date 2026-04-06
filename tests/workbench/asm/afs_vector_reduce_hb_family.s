// Workbench note: Apple `as` encoding pin for the narrower integer reduction
// family clang emitted from Neon intrinsics. This informed the `addv` /
// `smaxv` / `umaxv` / `sminv` / `uminv` `.16b` and `.8h` implementation.
.text
addv.16b b0, v0
addv.8h h0, v0
umaxv.16b b0, v0
smaxv.16b b0, v0
umaxv.8h h0, v0
smaxv.8h h0, v0
uminv.16b b0, v0
sminv.16b b0, v0
uminv.8h h0, v0
sminv.8h h0, v0
