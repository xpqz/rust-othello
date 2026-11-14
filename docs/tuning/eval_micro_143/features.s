10000e244:     	sub	sp, sp, #0xf0
10000e248:     	stp	x28, x27, [sp, #0x90]
10000e24c:     	stp	x26, x25, [sp, #0xa0]
10000e250:     	stp	x24, x23, [sp, #0xb0]
10000e254:     	stp	x22, x21, [sp, #0xc0]
10000e258:     	stp	x20, x19, [sp, #0xd0]
10000e25c:     	stp	x29, x30, [sp, #0xe0]
10000e260:     	add	x29, sp, #0xe0
10000e264:     	mov	x23, #0x0               ; =0
10000e268:     	mov	x24, #0x0               ; =0
10000e26c:     	movi.2d	v4, #0000000000000000
10000e270:     	adrp	x25, 0x100420000 <dyld_stub_binder+0x100420000>
10000e274:     	adrp	x26, 0x100420000 <dyld_stub_binder+0x100420000>
10000e278:     	mov	w27, #0x60              ; =96
10000e27c:     	sub	x11, x29, #0x60
10000e280:     	movi.2d	v5, #0000000000000000
10000e284:     	adrp	x19, 0x100420000 <dyld_stub_binder+0x100420000>
10000e288:     	add	x19, x19, #0xc70
10000e28c:     	movi.2d	v6, #0000000000000000
10000e290:     	movi.2d	v7, #0000000000000000
10000e294:     	adrp	x20, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000e298:     	add	x20, x20, #0x598
10000e29c:     	movi.2d	v16, #0000000000000000
10000e2a0:     	movi.2d	v17, #0000000000000000
10000e2a4:     	adrp	x21, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000e2a8:     	add	x21, x21, #0x580
10000e2ac:     	ldapr	x8, [x19]
10000e2b0:     	cbnz	x8, 0x10000e368 <__RNvNtCsfohv7ss3IV4_7othello7pattern8features+0x124>
10000e2b4:     	lsr	x8, x1, x24
10000e2b8:     	add	x22, x23, w8, uxtb
10000e2bc:     	ldr	x8, [x25, #0xc88]
10000e2c0:     	cmp	x22, x8
10000e2c4:     	b.hs	0x10000e53c <__RNvNtCsfohv7ss3IV4_7othello7pattern8features+0x2f8>
10000e2c8:     	ldr	x28, [x19, #0x10]
10000e2cc:     	ldapr	x8, [x19]
10000e2d0:     	cbnz	x8, 0x10000e3d0 <__RNvNtCsfohv7ss3IV4_7othello7pattern8features+0x18c>
10000e2d4:     	lsr	x8, x2, x24
10000e2d8:     	add	x8, x23, w8, uxtb
10000e2dc:     	ldr	x9, [x25, #0xc88]
10000e2e0:     	cmp	x8, x9
10000e2e4:     	b.hs	0x10000e42c <__RNvNtCsfohv7ss3IV4_7othello7pattern8features+0x1e8>
10000e2e8:     	ldr	x9, [x26, #0xc80]
10000e2ec:     	madd	x10, x22, x27, x28
10000e2f0:     	madd	x8, x8, x27, x9
10000e2f4:     	ldp	q0, q1, [x10]
10000e2f8:     	ldp	q2, q3, [x8]
10000e2fc:     	add.8h	v2, v2, v2
10000e300:     	add.8h	v3, v3, v3
10000e304:     	add.8h	v0, v0, v17
10000e308:     	add.8h	v17, v2, v0
10000e30c:     	add.8h	v0, v1, v16
10000e310:     	add.8h	v16, v3, v0
10000e314:     	ldp	q0, q1, [x10, #0x20]
10000e318:     	ldp	q2, q3, [x8, #0x20]
10000e31c:     	add.8h	v2, v2, v2
10000e320:     	add.8h	v3, v3, v3
10000e324:     	add.8h	v0, v0, v7
10000e328:     	add.8h	v7, v2, v0
10000e32c:     	add.8h	v0, v1, v6
10000e330:     	add.8h	v6, v3, v0
10000e334:     	ldp	q0, q1, [x10, #0x40]
10000e338:     	ldp	q2, q3, [x8, #0x40]
10000e33c:     	add.8h	v2, v2, v2
10000e340:     	add.8h	v3, v3, v3
10000e344:     	add.8h	v0, v0, v5
10000e348:     	add.8h	v5, v2, v0
10000e34c:     	add.8h	v0, v1, v4
10000e350:     	add.8h	v4, v3, v0
10000e354:     	add	x24, x24, #0x8
10000e358:     	add	x23, x23, #0x100
10000e35c:     	cmp	x24, #0x40
10000e360:     	b.ne	0x10000e2ac <__RNvNtCsfohv7ss3IV4_7othello7pattern8features+0x68>
10000e364:     	b	0x10000e440 <__RNvNtCsfohv7ss3IV4_7othello7pattern8features+0x1fc>
10000e368:     	stp	x19, x11, [x29, #-0x60]
10000e36c:     	str	x2, [sp, #0x18]
10000e370:     	sub	x2, x29, #0x58
10000e374:     	mov	x28, x0
10000e378:     	mov	x0, x19
10000e37c:     	mov	x22, x1
10000e380:     	mov	w1, #0x1                ; =1
10000e384:     	mov	x3, x20
10000e388:     	mov	x4, x21
10000e38c:     	stp	q5, q4, [sp, #0x60]
10000e390:     	stp	q7, q6, [sp, #0x40]
10000e394:     	stp	q17, q16, [sp, #0x20]
10000e398:     	bl	0x100039358 <__RNvMNtNtNtNtCsdnanKjkF2cC_3std3sys4sync4once5queueNtB2_4Once4call>
10000e39c:     	sub	x11, x29, #0x60
10000e3a0:     	ldp	q17, q16, [sp, #0x20]
10000e3a4:     	ldp	q7, q6, [sp, #0x40]
10000e3a8:     	ldp	q5, q4, [sp, #0x60]
10000e3ac:     	mov	x1, x22
10000e3b0:     	ldr	x2, [sp, #0x18]
10000e3b4:     	mov	x0, x28
10000e3b8:     	lsr	x8, x22, x24
10000e3bc:     	add	x22, x23, w8, uxtb
10000e3c0:     	ldr	x8, [x25, #0xc88]
10000e3c4:     	cmp	x22, x8
10000e3c8:     	b.lo	0x10000e2c8 <__RNvNtCsfohv7ss3IV4_7othello7pattern8features+0x84>
10000e3cc:     	b	0x10000e53c <__RNvNtCsfohv7ss3IV4_7othello7pattern8features+0x2f8>
10000e3d0:     	stp	x19, x11, [x29, #-0x60]
10000e3d4:     	stp	x0, x2, [sp, #0x10]
10000e3d8:     	sub	x2, x29, #0x58
10000e3dc:     	mov	x0, x19
10000e3e0:     	str	x1, [sp, #0x8]
10000e3e4:     	mov	w1, #0x1                ; =1
10000e3e8:     	mov	x3, x20
10000e3ec:     	mov	x4, x21
10000e3f0:     	stp	q5, q4, [sp, #0x60]
10000e3f4:     	stp	q7, q6, [sp, #0x40]
10000e3f8:     	stp	q17, q16, [sp, #0x20]
10000e3fc:     	bl	0x100039358 <__RNvMNtNtNtNtCsdnanKjkF2cC_3std3sys4sync4once5queueNtB2_4Once4call>
10000e400:     	sub	x11, x29, #0x60
10000e404:     	ldp	q17, q16, [sp, #0x20]
10000e408:     	ldp	q7, q6, [sp, #0x40]
10000e40c:     	ldp	q5, q4, [sp, #0x60]
10000e410:     	ldp	x1, x0, [sp, #0x8]
10000e414:     	ldr	x2, [sp, #0x18]
10000e418:     	lsr	x8, x2, x24
10000e41c:     	add	x8, x23, w8, uxtb
10000e420:     	ldr	x9, [x25, #0xc88]
10000e424:     	cmp	x8, x9
10000e428:     	b.lo	0x10000e2e8 <__RNvNtCsfohv7ss3IV4_7othello7pattern8features+0xa4>
10000e42c:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000e430:     	add	x2, x2, #0x840
10000e434:     	mov	x0, x8
10000e438:     	mov	x1, x9
10000e43c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
10000e440:     	adrp	x8, 0x100420000 <dyld_stub_binder+0x100420000>
10000e444:     	add	x8, x8, #0xcd8
10000e448:     	ldapr	x9, [x8]
10000e44c:     	cbnz	x9, 0x10000e4f0 <__RNvNtCsfohv7ss3IV4_7othello7pattern8features+0x2ac>
10000e450:     	adrp	x8, 0x100420000 <dyld_stub_binder+0x100420000>
10000e454:     	add	x8, x8, #0xce0
10000e458:     	ldp	q0, q1, [x8]
10000e45c:     	uaddw.4s	v0, v0, v17
10000e460:     	uaddw2.4s	v1, v1, v17
10000e464:     	stp	q0, q1, [x0]
10000e468:     	ldp	q0, q1, [x8, #0x20]
10000e46c:     	uaddw.4s	v0, v0, v16
10000e470:     	uaddw2.4s	v1, v1, v16
10000e474:     	stp	q0, q1, [x0, #0x20]
10000e478:     	ldp	q0, q1, [x8, #0x40]
10000e47c:     	uaddw.4s	v0, v0, v7
10000e480:     	uaddw2.4s	v1, v1, v7
10000e484:     	stp	q0, q1, [x0, #0x40]
10000e488:     	ldp	q0, q1, [x8, #0x60]
10000e48c:     	uaddw.4s	v0, v0, v6
10000e490:     	uaddw2.4s	v1, v1, v6
10000e494:     	stp	q0, q1, [x0, #0x60]
10000e498:     	ldp	q0, q1, [x8, #0x80]
10000e49c:     	uaddw.4s	v0, v0, v5
10000e4a0:     	uaddw2.4s	v1, v1, v5
10000e4a4:     	stp	q0, q1, [x0, #0x80]
10000e4a8:     	ldr	q0, [x8, #0xa0]
10000e4ac:     	uaddw.4s	v0, v0, v4
10000e4b0:     	str	q0, [x0, #0xa0]
10000e4b4:     	ldr	d0, [x8, #0xb0]
10000e4b8:     	mov	h1, v4[4]
10000e4bc:     	mov.h	v1[2], v4[5]
10000e4c0:     	movi	d2, #0x00ffff0000ffff
10000e4c4:     	and.8b	v1, v1, v2
10000e4c8:     	add.2s	v0, v0, v1
10000e4cc:     	str	d0, [x0, #0xb0]
10000e4d0:     	ldp	x29, x30, [sp, #0xe0]
10000e4d4:     	ldp	x20, x19, [sp, #0xd0]
10000e4d8:     	ldp	x22, x21, [sp, #0xc0]
10000e4dc:     	ldp	x24, x23, [sp, #0xb0]
10000e4e0:     	ldp	x26, x25, [sp, #0xa0]
10000e4e4:     	ldp	x28, x27, [sp, #0x90]
10000e4e8:     	add	sp, sp, #0xf0
10000e4ec:     	ret
10000e4f0:     	sub	x9, x29, #0x60
10000e4f4:     	stp	x8, x9, [x29, #-0x60]
10000e4f8:     	adrp	x3, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000e4fc:     	add	x3, x3, #0x558
10000e500:     	adrp	x4, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000e504:     	add	x4, x4, #0x580
10000e508:     	sub	x2, x29, #0x58
10000e50c:     	mov	x19, x0
10000e510:     	mov	x0, x8
10000e514:     	mov	w1, #0x1                ; =1
10000e518:     	stp	q5, q4, [sp, #0x60]
10000e51c:     	stp	q7, q6, [sp, #0x40]
10000e520:     	stp	q17, q16, [sp, #0x20]
10000e524:     	bl	0x100039358 <__RNvMNtNtNtNtCsdnanKjkF2cC_3std3sys4sync4once5queueNtB2_4Once4call>
10000e528:     	ldp	q17, q16, [sp, #0x20]
10000e52c:     	ldp	q7, q6, [sp, #0x40]
10000e530:     	ldp	q5, q4, [sp, #0x60]
10000e534:     	mov	x0, x19
10000e538:     	b	0x10000e450 <__RNvNtCsfohv7ss3IV4_7othello7pattern8features+0x20c>
10000e53c:     	adrp	x2, 0x10041c000 <dyld_stub_binder+0x10041c000>
10000e540:     	add	x2, x2, #0x828
10000e544:     	mov	x0, x22
10000e548:     	mov	x1, x8
10000e54c:     	bl	0x100034f60 <__RNvNtCs3bJEMBXHbwl_4core9panicking18panic_bounds_check>
