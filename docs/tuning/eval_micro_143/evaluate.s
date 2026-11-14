10000be40:     	stp	x28, x27, [sp, #-0x60]!
10000be44:     	stp	x26, x25, [sp, #0x10]
10000be48:     	stp	x24, x23, [sp, #0x20]
10000be4c:     	stp	x22, x21, [sp, #0x30]
10000be50:     	stp	x20, x19, [sp, #0x40]
10000be54:     	stp	x29, x30, [sp, #0x50]
10000be58:     	add	x29, sp, #0x50
10000be5c:     	sub	sp, sp, #0x1a0
10000be60:     	mov	x20, x1
10000be64:     	mov	x21, x0
10000be68:     	orr	x8, x1, x0
10000be6c:     	fmov	d0, x8
10000be70:     	cnt.8b	v0, v0
10000be74:     	addv.8b	b0, v0
10000be78:     	fmov	w8, s0
10000be7c:     	mov	w9, #0x40               ; =64
10000be80:     	mov	w10, #0x3c              ; =60
10000be84:     	sub	w8, w9, w8
10000be88:     	subs	w8, w10, w8
10000be8c:     	csel	w8, wzr, w8, lo
10000be90:     	mov	w9, #0x34               ; =52
10000be94:     	mul	w8, w8, w9
10000be98:     	lsr	w8, w8, #8
10000be9c:     	mov	w9, #0xb                ; =11
10000bea0:     	cmp	w8, #0xb
10000bea4:     	csel	w19, w8, w9, lo
10000bea8:     	adrp	x0, 0x100420000 <dyld_stub_binder+0x100420000>
10000beac:     	add	x0, x0, #0x238
10000beb0:     	ldapr	x8, [x0]
10000beb4:     	cbnz	x8, 0x10000c388 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x548>
10000beb8:     	mov	w8, #0x8d61             ; =36193
10000bebc:     	movk	w8, #0x2, lsl #16
10000bec0:     	umull	x24, w19, w8
10000bec4:     	adrp	x9, 0x100420000 <dyld_stub_binder+0x100420000>
10000bec8:     	ldr	x1, [x9, #0x250]
10000becc:     	subs	x2, x1, x24
10000bed0:     	b.lo	0x10000c3b4 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x574>
10000bed4:     	sub	x8, x8, #0x1
10000bed8:     	cmp	x2, x8
10000bedc:     	b.ls	0x10000c3c8 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x588>
10000bee0:     	adrp	x8, 0x100420000 <dyld_stub_binder+0x100420000>
10000bee4:     	ldr	x19, [x8, #0x248]
10000bee8:     	mov	w23, #0x8d61            ; =36193
10000beec:     	movk	w23, #0x2, lsl #16
10000bef0:     	add	x0, sp, #0xd8
10000bef4:     	mov	x1, x21
10000bef8:     	mov	x2, x20
10000befc:     	bl	0x10000e244 <__RNvNtCsfohv7ss3IV4_7othello7pattern8features>
10000bf00:     	ldr	w8, [sp, #0xd8]
10000bf04:     	cmp	w8, w23
10000bf08:     	b.hs	0x10000c3e0 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x5a0>
10000bf0c:     	ldr	w9, [sp, #0xdc]
10000bf10:     	cmp	w9, w23
10000bf14:     	b.hs	0x10000c3f8 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x5b8>
10000bf18:     	ldr	w7, [sp, #0xe0]
10000bf1c:     	cmp	w7, w23
10000bf20:     	b.hs	0x10000c458 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x618>
10000bf24:     	ldr	w6, [sp, #0xe4]
10000bf28:     	cmp	w6, w23
10000bf2c:     	b.hs	0x10000c470 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x630>
10000bf30:     	ldr	w27, [sp, #0xe8]
10000bf34:     	cmp	w27, w23
10000bf38:     	b.hs	0x10000c488 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x648>
10000bf3c:     	ldr	w28, [sp, #0xec]
10000bf40:     	cmp	w28, w23
10000bf44:     	b.hs	0x10000c4a0 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x660>
10000bf48:     	ldr	w26, [sp, #0xf0]
10000bf4c:     	cmp	w26, w23
10000bf50:     	b.hs	0x10000c4b8 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x678>
10000bf54:     	ldr	w22, [sp, #0xf4]
10000bf58:     	cmp	w22, w23
10000bf5c:     	b.hs	0x10000c4d0 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x690>
10000bf60:     	ldr	w12, [sp, #0xf8]
10000bf64:     	cmp	w12, w23
10000bf68:     	b.hs	0x10000c4e8 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x6a8>
10000bf6c:     	ldr	w13, [sp, #0xfc]
10000bf70:     	cmp	w13, w23
10000bf74:     	b.hs	0x10000c500 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x6c0>
10000bf78:     	ldr	w14, [sp, #0x100]
10000bf7c:     	cmp	w14, w23
10000bf80:     	b.hs	0x10000c518 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x6d8>
10000bf84:     	ldr	w15, [sp, #0x104]
10000bf88:     	cmp	w15, w23
10000bf8c:     	b.hs	0x10000c530 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x6f0>
10000bf90:     	ldr	w16, [sp, #0x108]
10000bf94:     	cmp	w16, w23
10000bf98:     	b.hs	0x10000c548 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x708>
10000bf9c:     	ldr	w17, [sp, #0x10c]
10000bfa0:     	cmp	w17, w23
10000bfa4:     	b.hs	0x10000c560 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x720>
10000bfa8:     	ldr	w1, [sp, #0x110]
10000bfac:     	cmp	w1, w23
10000bfb0:     	b.hs	0x10000c578 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x738>
10000bfb4:     	ldr	w2, [sp, #0x114]
10000bfb8:     	cmp	w2, w23
10000bfbc:     	b.hs	0x10000c590 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x750>
10000bfc0:     	ldr	w3, [sp, #0x118]
10000bfc4:     	cmp	w3, w23
10000bfc8:     	b.hs	0x10000c5a8 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x768>
10000bfcc:     	ldr	w4, [sp, #0x11c]
10000bfd0:     	cmp	w4, w23
10000bfd4:     	b.hs	0x10000c5c0 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x780>
10000bfd8:     	ldr	w5, [sp, #0x120]
10000bfdc:     	cmp	w5, w23
10000bfe0:     	b.hs	0x10000c5d8 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x798>
10000bfe4:     	ldr	w30, [sp, #0x124]
10000bfe8:     	cmp	w30, w23
10000bfec:     	b.hs	0x10000c5f0 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x7b0>
10000bff0:     	str	x19, [sp, #0xd0]
10000bff4:     	ldr	w11, [sp, #0x128]
10000bff8:     	cmp	w11, w23
10000bffc:     	b.hs	0x10000c428 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x5e8>
10000c000:     	ldr	w19, [sp, #0x12c]
10000c004:     	cmp	w19, w23
10000c008:     	b.hs	0x10000c440 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x600>
10000c00c:     	ldr	w0, [sp, #0x130]
10000c010:     	cmp	w0, w23
10000c014:     	b.hs	0x10000c744 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x904>
10000c018:     	stp	x24, x9, [sp, #0xc0]
10000c01c:     	ldr	w9, [sp, #0x134]
10000c020:     	cmp	w9, w23
10000c024:     	b.hs	0x10000c3f8 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x5b8>
10000c028:     	str	x8, [sp, #0xb8]
10000c02c:     	ldr	w10, [sp, #0x138]
10000c030:     	cmp	w10, w23
10000c034:     	b.hs	0x10000c410 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x5d0>
10000c038:     	ldr	w8, [sp, #0x13c]
10000c03c:     	cmp	w8, w23
10000c040:     	b.hs	0x10000c3e0 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x5a0>
10000c044:     	ldr	w25, [sp, #0x140]
10000c048:     	str	x25, [sp, #0xb0]
10000c04c:     	cmp	w25, w23
10000c050:     	b.hs	0x10000c608 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x7c8>
10000c054:     	ldr	w25, [sp, #0x144]
10000c058:     	cmp	w25, w23
10000c05c:     	b.hs	0x10000c620 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x7e0>
10000c060:     	str	x8, [sp, #0xa8]
10000c064:     	ldr	w8, [sp, #0x148]
10000c068:     	cmp	w8, w23
10000c06c:     	b.hs	0x10000c3e0 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x5a0>
10000c070:     	ldr	w20, [sp, #0x14c]
10000c074:     	str	x20, [sp, #0xa0]
10000c078:     	cmp	w20, w23
10000c07c:     	b.hs	0x10000c638 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x7f8>
10000c080:     	stp	x10, x9, [sp, #0x90]
10000c084:     	ldr	w20, [sp, #0x150]
10000c088:     	cmp	w20, w23
10000c08c:     	b.hs	0x10000c650 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x810>
10000c090:     	ldr	w9, [sp, #0x154]
10000c094:     	cmp	w9, w23
10000c098:     	b.hs	0x10000c3f8 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x5b8>
10000c09c:     	ldr	w10, [sp, #0x158]
10000c0a0:     	cmp	w10, w23
10000c0a4:     	b.hs	0x10000c410 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x5d0>
10000c0a8:     	ldr	w21, [sp, #0x15c]
10000c0ac:     	str	x21, [sp, #0x88]
10000c0b0:     	cmp	w21, w23
10000c0b4:     	b.hs	0x10000c668 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x828>
10000c0b8:     	ldr	w21, [sp, #0x160]
10000c0bc:     	cmp	w21, w23
10000c0c0:     	b.hs	0x10000c680 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x840>
10000c0c4:     	ldr	w24, [sp, #0x164]
10000c0c8:     	str	x24, [sp, #0x80]
10000c0cc:     	cmp	w24, w23
10000c0d0:     	b.hs	0x10000c698 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x858>
10000c0d4:     	ldr	w24, [sp, #0x168]
10000c0d8:     	cmp	w24, w23
10000c0dc:     	b.hs	0x10000c6b0 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x870>
10000c0e0:     	str	x0, [sp, #0x70]
10000c0e4:     	str	x19, [sp, #0x60]
10000c0e8:     	ldr	w0, [sp, #0x16c]
10000c0ec:     	str	x0, [sp, #0x78]
10000c0f0:     	cmp	w0, w23
10000c0f4:     	b.hs	0x10000c6c8 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x888>
10000c0f8:     	str	x11, [sp, #0x58]
10000c0fc:     	ldr	w11, [sp, #0x170]
10000c100:     	str	x11, [sp, #0x68]
10000c104:     	cmp	w11, w23
10000c108:     	b.hs	0x10000c6e0 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x8a0>
10000c10c:     	ldr	w11, [sp, #0x174]
10000c110:     	cmp	w11, w23
10000c114:     	b.hs	0x10000c428 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x5e8>
10000c118:     	ldr	w0, [sp, #0x178]
10000c11c:     	str	x0, [sp, #0x50]
10000c120:     	cmp	w0, w23
10000c124:     	b.hs	0x10000c6f8 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x8b8>
10000c128:     	ldr	w0, [sp, #0x17c]
10000c12c:     	cmp	w0, w23
10000c130:     	b.hs	0x10000c744 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x904>
10000c134:     	ldr	w19, [sp, #0x180]
10000c138:     	str	x19, [sp, #0x48]
10000c13c:     	cmp	w19, w23
10000c140:     	b.hs	0x10000c710 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x8d0>
10000c144:     	ldr	w19, [sp, #0x184]
10000c148:     	cmp	w19, w23
10000c14c:     	b.hs	0x10000c440 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x600>
10000c150:     	stp	x10, x9, [sp, #0x30]
10000c154:     	mov	x9, x8
10000c158:     	ldr	w8, [sp, #0x188]
10000c15c:     	str	x8, [sp, #0x40]
10000c160:     	cmp	w8, w23
10000c164:     	b.hs	0x10000c728 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x8e8>
10000c168:     	stp	x19, x0, [sp, #0x8]
10000c16c:     	stp	x24, x21, [sp, #0x18]
10000c170:     	ldr	w0, [sp, #0x18c]
10000c174:     	str	x0, [sp, #0x28]
10000c178:     	cmp	w0, w23
10000c17c:     	b.hs	0x10000c740 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x900>
10000c180:     	mov	x10, x20
10000c184:     	mov	x8, x25
10000c188:     	ldr	x20, [sp, #0xd0]
10000c18c:     	ldp	x0, x21, [sp, #0xc0]
10000c190:     	add	x23, x20, x0, lsl #1
10000c194:     	ldr	x20, [sp, #0xb8]
10000c198:     	ldrsh	w20, [x23, x20, lsl #1]
10000c19c:     	ldrsh	w21, [x23, x21, lsl #1]
10000c1a0:     	ldrsh	w24, [x23, x7, lsl #1]
10000c1a4:     	add	w20, w20, w21
10000c1a8:     	ldrsh	w21, [x23, x6, lsl #1]
10000c1ac:     	add	w21, w24, w21
10000c1b0:     	ldrsh	w24, [x23, x27, lsl #1]
10000c1b4:     	add	w20, w20, w21
10000c1b8:     	ldrsh	w21, [x23, x28, lsl #1]
10000c1bc:     	ldrsh	w25, [x23, x26, lsl #1]
10000c1c0:     	add	w21, w24, w21
10000c1c4:     	add	w21, w21, w25
10000c1c8:     	add	w20, w20, w21
10000c1cc:     	ldrsh	w21, [x23, x22, lsl #1]
10000c1d0:     	ldrsh	w24, [x23, x12, lsl #1]
10000c1d4:     	ldrsh	w25, [x23, x13, lsl #1]
10000c1d8:     	add	w21, w21, w24
10000c1dc:     	add	w21, w21, w25
10000c1e0:     	ldrsh	w24, [x23, x14, lsl #1]
10000c1e4:     	add	w21, w21, w24
10000c1e8:     	add	w20, w20, w21
10000c1ec:     	ldrsh	w21, [x23, x15, lsl #1]
10000c1f0:     	ldrsh	w24, [x23, x16, lsl #1]
10000c1f4:     	ldrsh	w25, [x23, x17, lsl #1]
10000c1f8:     	add	w21, w21, w24
10000c1fc:     	add	w21, w21, w25
10000c200:     	ldrsh	w24, [x23, x1, lsl #1]
10000c204:     	ldrsh	w25, [x23, x2, lsl #1]
10000c208:     	add	w21, w21, w24
10000c20c:     	add	w21, w21, w25
10000c210:     	ldrsh	w24, [x23, x3, lsl #1]
10000c214:     	add	w20, w20, w21
10000c218:     	ldrsh	w21, [x23, x4, lsl #1]
10000c21c:     	ldrsh	w25, [x23, x5, lsl #1]
10000c220:     	add	w21, w24, w21
10000c224:     	add	w21, w21, w25
10000c228:     	ldrsh	w24, [x23, x30, lsl #1]
10000c22c:     	ldp	x13, x12, [sp, #0x58]
10000c230:     	ldrsh	w25, [x23, x13, lsl #1]
10000c234:     	add	w21, w21, w24
10000c238:     	add	w21, w21, w25
10000c23c:     	ldrsh	w24, [x23, x12, lsl #1]
10000c240:     	ldr	x12, [sp, #0x70]
10000c244:     	ldrsh	w25, [x23, x12, lsl #1]
10000c248:     	add	w21, w21, w24
10000c24c:     	add	w20, w20, w21
10000c250:     	ldp	x12, x13, [sp, #0x90]
10000c254:     	ldrsh	w21, [x23, x13, lsl #1]
10000c258:     	ldrsh	w24, [x23, x12, lsl #1]
10000c25c:     	add	w21, w25, w21
10000c260:     	add	w21, w21, w24
10000c264:     	ldp	x13, x12, [sp, #0xa8]
10000c268:     	ldrsh	w24, [x23, x13, lsl #1]
10000c26c:     	ldrsh	w0, [x23, x12, lsl #1]
10000c270:     	add	w21, w21, w24
10000c274:     	add	w0, w21, w0
10000c278:     	ldrsh	w21, [x23, x8, lsl #1]
10000c27c:     	ldrsh	w19, [x23, x9, lsl #1]
10000c280:     	add	w0, w0, w21
10000c284:     	add	w0, w0, w19
10000c288:     	add	w0, w20, w0
10000c28c:     	ldr	x8, [sp, #0xa0]
10000c290:     	ldrsh	w8, [x23, x8, lsl #1]
10000c294:     	ldrsh	w9, [x23, x10, lsl #1]
10000c298:     	ldr	x10, [sp, #0x38]
10000c29c:     	ldrsh	w10, [x23, x10, lsl #1]
10000c2a0:     	add	w8, w8, w9
10000c2a4:     	add	w8, w8, w10
10000c2a8:     	ldr	x9, [sp, #0x30]
10000c2ac:     	ldrsh	w9, [x23, x9, lsl #1]
10000c2b0:     	ldr	x10, [sp, #0x88]
10000c2b4:     	ldrsh	w10, [x23, x10, lsl #1]
10000c2b8:     	add	w8, w8, w9
10000c2bc:     	add	w8, w8, w10
10000c2c0:     	ldr	x9, [sp, #0x20]
10000c2c4:     	ldrsh	w9, [x23, x9, lsl #1]
10000c2c8:     	ldr	x10, [sp, #0x80]
10000c2cc:     	ldrsh	w10, [x23, x10, lsl #1]
10000c2d0:     	add	w8, w8, w9
10000c2d4:     	add	w8, w8, w10
10000c2d8:     	ldr	x9, [sp, #0x18]
10000c2dc:     	ldrsh	w9, [x23, x9, lsl #1]
10000c2e0:     	add	w8, w8, w9
10000c2e4:     	add	w8, w0, w8
10000c2e8:     	ldr	x9, [sp, #0x78]
10000c2ec:     	ldrsh	w9, [x23, x9, lsl #1]
10000c2f0:     	ldr	x10, [sp, #0x68]
10000c2f4:     	ldrsh	w10, [x23, x10, lsl #1]
10000c2f8:     	ldrsh	w11, [x23, x11, lsl #1]
10000c2fc:     	add	w9, w9, w10
10000c300:     	add	w9, w9, w11
10000c304:     	ldr	x10, [sp, #0x50]
10000c308:     	ldrsh	w10, [x23, x10, lsl #1]
10000c30c:     	ldr	x11, [sp, #0x10]
10000c310:     	ldrsh	w11, [x23, x11, lsl #1]
10000c314:     	add	w9, w9, w10
10000c318:     	add	w9, w9, w11
10000c31c:     	ldr	x10, [sp, #0x48]
10000c320:     	ldrsh	w10, [x23, x10, lsl #1]
10000c324:     	ldr	x11, [sp, #0x8]
10000c328:     	ldrsh	w11, [x23, x11, lsl #1]
10000c32c:     	add	w9, w9, w10
10000c330:     	add	w9, w9, w11
10000c334:     	ldr	x10, [sp, #0x40]
10000c338:     	ldrsh	w10, [x23, x10, lsl #1]
10000c33c:     	ldr	x11, [sp, #0x28]
10000c340:     	ldrsh	w11, [x23, x11, lsl #1]
10000c344:     	add	w9, w9, w10
10000c348:     	add	w9, w9, w11
10000c34c:     	mov	w10, #-0x1900           ; =-6400
10000c350:     	add	w8, w8, w9
10000c354:     	cmp	w8, w10
10000c358:     	csel	w8, w8, w10, gt
10000c35c:     	mov	w9, #0x1900             ; =6400
10000c360:     	cmp	w8, w9
10000c364:     	csel	w0, w8, w9, lt
10000c368:     	add	sp, sp, #0x1a0
10000c36c:     	ldp	x29, x30, [sp, #0x50]
10000c370:     	ldp	x20, x19, [sp, #0x40]
10000c374:     	ldp	x22, x21, [sp, #0x30]
10000c378:     	ldp	x24, x23, [sp, #0x20]
10000c37c:     	ldp	x26, x25, [sp, #0x10]
10000c380:     	ldp	x28, x27, [sp], #0x60
10000c384:     	ret
10000c388:     	stur	x0, [x29, #-0x60]
10000c38c:     	sub	x8, x29, #0x60
10000c390:     	str	x8, [sp, #0xd8]
10000c394:     	adrp	x3, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c398:     	add	x3, x3, #0x5e8
10000c39c:     	adrp	x4, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c3a0:     	add	x4, x4, #0x580
10000c3a4:     	add	x2, sp, #0xd8
10000c3a8:     	mov	w1, #0x1                ; =1
10000c3ac:     	bl	0x100039358 <__RNvMNtNtNtNtCsdnanKjkF2cC_3std3sys4sync4once5queueNtB2_4Once4call>
10000c3b0:     	b	0x10000beb8 <__RNvNtCsfohv7ss3IV4_7othello7pattern8evaluate+0x78>
10000c3b4:     	adrp	x3, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c3b8:     	add	x3, x3, #0x810
10000c3bc:     	mov	x0, x24
10000c3c0:     	mov	x2, x1
10000c3c4:     	bl	0x100034e98 <__RNvNtNtCs3bJEMBXHbwl_4core5slice5index16slice_index_fail>
10000c3c8:     	adrp	x3, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c3cc:     	add	x3, x3, #0x7e0
10000c3d0:     	mov	x0, #0x0                ; =0
10000c3d4:     	mov	w1, #0x8d61             ; =36193
10000c3d8:     	movk	w1, #0x2, lsl #16
10000c3dc:     	bl	0x100034e98 <__RNvNtNtCs3bJEMBXHbwl_4core5slice5index16slice_index_fail>
10000c3e0:     	mov	x0, x8
10000c3e4:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c3e8:     	add	x2, x2, #0x7f8
10000c3ec:     	mov	w1, #0x8d61             ; =36193
10000c3f0:     	movk	w1, #0x2, lsl #16
10000c3f4:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c3f8:     	mov	x0, x9
10000c3fc:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c400:     	add	x2, x2, #0x7f8
10000c404:     	mov	w1, #0x8d61             ; =36193
10000c408:     	movk	w1, #0x2, lsl #16
10000c40c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c410:     	mov	x0, x10
10000c414:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c418:     	add	x2, x2, #0x7f8
10000c41c:     	mov	w1, #0x8d61             ; =36193
10000c420:     	movk	w1, #0x2, lsl #16
10000c424:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c428:     	mov	x0, x11
10000c42c:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c430:     	add	x2, x2, #0x7f8
10000c434:     	mov	w1, #0x8d61             ; =36193
10000c438:     	movk	w1, #0x2, lsl #16
10000c43c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c440:     	mov	x0, x19
10000c444:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c448:     	add	x2, x2, #0x7f8
10000c44c:     	mov	w1, #0x8d61             ; =36193
10000c450:     	movk	w1, #0x2, lsl #16
10000c454:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c458:     	mov	x0, x7
10000c45c:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c460:     	add	x2, x2, #0x7f8
10000c464:     	mov	w1, #0x8d61             ; =36193
10000c468:     	movk	w1, #0x2, lsl #16
10000c46c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c470:     	mov	x0, x6
10000c474:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c478:     	add	x2, x2, #0x7f8
10000c47c:     	mov	w1, #0x8d61             ; =36193
10000c480:     	movk	w1, #0x2, lsl #16
10000c484:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c488:     	mov	x0, x27
10000c48c:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c490:     	add	x2, x2, #0x7f8
10000c494:     	mov	w1, #0x8d61             ; =36193
10000c498:     	movk	w1, #0x2, lsl #16
10000c49c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c4a0:     	mov	x0, x28
10000c4a4:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c4a8:     	add	x2, x2, #0x7f8
10000c4ac:     	mov	w1, #0x8d61             ; =36193
10000c4b0:     	movk	w1, #0x2, lsl #16
10000c4b4:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c4b8:     	mov	x0, x26
10000c4bc:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c4c0:     	add	x2, x2, #0x7f8
10000c4c4:     	mov	w1, #0x8d61             ; =36193
10000c4c8:     	movk	w1, #0x2, lsl #16
10000c4cc:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c4d0:     	mov	x0, x22
10000c4d4:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c4d8:     	add	x2, x2, #0x7f8
10000c4dc:     	mov	w1, #0x8d61             ; =36193
10000c4e0:     	movk	w1, #0x2, lsl #16
10000c4e4:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c4e8:     	mov	x0, x12
10000c4ec:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c4f0:     	add	x2, x2, #0x7f8
10000c4f4:     	mov	w1, #0x8d61             ; =36193
10000c4f8:     	movk	w1, #0x2, lsl #16
10000c4fc:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c500:     	mov	x0, x13
10000c504:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c508:     	add	x2, x2, #0x7f8
10000c50c:     	mov	w1, #0x8d61             ; =36193
10000c510:     	movk	w1, #0x2, lsl #16
10000c514:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c518:     	mov	x0, x14
10000c51c:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c520:     	add	x2, x2, #0x7f8
10000c524:     	mov	w1, #0x8d61             ; =36193
10000c528:     	movk	w1, #0x2, lsl #16
10000c52c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c530:     	mov	x0, x15
10000c534:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c538:     	add	x2, x2, #0x7f8
10000c53c:     	mov	w1, #0x8d61             ; =36193
10000c540:     	movk	w1, #0x2, lsl #16
10000c544:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c548:     	mov	x0, x16
10000c54c:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c550:     	add	x2, x2, #0x7f8
10000c554:     	mov	w1, #0x8d61             ; =36193
10000c558:     	movk	w1, #0x2, lsl #16
10000c55c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c560:     	mov	x0, x17
10000c564:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c568:     	add	x2, x2, #0x7f8
10000c56c:     	mov	w1, #0x8d61             ; =36193
10000c570:     	movk	w1, #0x2, lsl #16
10000c574:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c578:     	mov	x0, x1
10000c57c:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c580:     	add	x2, x2, #0x7f8
10000c584:     	mov	w1, #0x8d61             ; =36193
10000c588:     	movk	w1, #0x2, lsl #16
10000c58c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c590:     	mov	x0, x2
10000c594:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c598:     	add	x2, x2, #0x7f8
10000c59c:     	mov	w1, #0x8d61             ; =36193
10000c5a0:     	movk	w1, #0x2, lsl #16
10000c5a4:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c5a8:     	mov	x0, x3
10000c5ac:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c5b0:     	add	x2, x2, #0x7f8
10000c5b4:     	mov	w1, #0x8d61             ; =36193
10000c5b8:     	movk	w1, #0x2, lsl #16
10000c5bc:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c5c0:     	mov	x0, x4
10000c5c4:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c5c8:     	add	x2, x2, #0x7f8
10000c5cc:     	mov	w1, #0x8d61             ; =36193
10000c5d0:     	movk	w1, #0x2, lsl #16
10000c5d4:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c5d8:     	mov	x0, x5
10000c5dc:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c5e0:     	add	x2, x2, #0x7f8
10000c5e4:     	mov	w1, #0x8d61             ; =36193
10000c5e8:     	movk	w1, #0x2, lsl #16
10000c5ec:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c5f0:     	mov	x0, x30
10000c5f4:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c5f8:     	add	x2, x2, #0x7f8
10000c5fc:     	mov	w1, #0x8d61             ; =36193
10000c600:     	movk	w1, #0x2, lsl #16
10000c604:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c608:     	ldr	x0, [sp, #0xb0]
10000c60c:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c610:     	add	x2, x2, #0x7f8
10000c614:     	mov	w1, #0x8d61             ; =36193
10000c618:     	movk	w1, #0x2, lsl #16
10000c61c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c620:     	mov	x0, x25
10000c624:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c628:     	add	x2, x2, #0x7f8
10000c62c:     	mov	w1, #0x8d61             ; =36193
10000c630:     	movk	w1, #0x2, lsl #16
10000c634:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c638:     	ldr	x0, [sp, #0xa0]
10000c63c:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c640:     	add	x2, x2, #0x7f8
10000c644:     	mov	w1, #0x8d61             ; =36193
10000c648:     	movk	w1, #0x2, lsl #16
10000c64c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c650:     	mov	x0, x20
10000c654:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c658:     	add	x2, x2, #0x7f8
10000c65c:     	mov	w1, #0x8d61             ; =36193
10000c660:     	movk	w1, #0x2, lsl #16
10000c664:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c668:     	ldr	x0, [sp, #0x88]
10000c66c:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c670:     	add	x2, x2, #0x7f8
10000c674:     	mov	w1, #0x8d61             ; =36193
10000c678:     	movk	w1, #0x2, lsl #16
10000c67c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c680:     	mov	x0, x21
10000c684:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c688:     	add	x2, x2, #0x7f8
10000c68c:     	mov	w1, #0x8d61             ; =36193
10000c690:     	movk	w1, #0x2, lsl #16
10000c694:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c698:     	ldr	x0, [sp, #0x80]
10000c69c:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c6a0:     	add	x2, x2, #0x7f8
10000c6a4:     	mov	w1, #0x8d61             ; =36193
10000c6a8:     	movk	w1, #0x2, lsl #16
10000c6ac:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c6b0:     	mov	x0, x24
10000c6b4:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c6b8:     	add	x2, x2, #0x7f8
10000c6bc:     	mov	w1, #0x8d61             ; =36193
10000c6c0:     	movk	w1, #0x2, lsl #16
10000c6c4:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c6c8:     	ldr	x0, [sp, #0x78]
10000c6cc:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c6d0:     	add	x2, x2, #0x7f8
10000c6d4:     	mov	w1, #0x8d61             ; =36193
10000c6d8:     	movk	w1, #0x2, lsl #16
10000c6dc:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c6e0:     	ldr	x0, [sp, #0x68]
10000c6e4:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c6e8:     	add	x2, x2, #0x7f8
10000c6ec:     	mov	w1, #0x8d61             ; =36193
10000c6f0:     	movk	w1, #0x2, lsl #16
10000c6f4:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c6f8:     	ldr	x0, [sp, #0x50]
10000c6fc:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c700:     	add	x2, x2, #0x7f8
10000c704:     	mov	w1, #0x8d61             ; =36193
10000c708:     	movk	w1, #0x2, lsl #16
10000c70c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c710:     	ldr	x0, [sp, #0x48]
10000c714:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c718:     	add	x2, x2, #0x7f8
10000c71c:     	mov	w1, #0x8d61             ; =36193
10000c720:     	movk	w1, #0x2, lsl #16
10000c724:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c728:     	ldr	x0, [sp, #0x40]
10000c72c:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c730:     	add	x2, x2, #0x7f8
10000c734:     	mov	w1, #0x8d61             ; =36193
10000c738:     	movk	w1, #0x2, lsl #16
10000c73c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000c740:     	ldr	x0, [sp, #0x28]
10000c744:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000c748:     	add	x2, x2, #0x7f8
10000c74c:     	mov	w1, #0x8d61             ; =36193
10000c750:     	movk	w1, #0x2, lsl #16
10000c754:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
