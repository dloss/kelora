t/checkout.t .. 
1..11
# ts=2026-07-01T08:10:01Z level=info msg="starting test server" port=18080
ok 1 - server starts
# ts=2026-07-01T08:10:02Z level=info msg="cart created" cart_id=c-1001 user=alice
# ts=2026-07-01T08:10:03Z level=debug msg="added item" cart_id=c-1001 qty=2 sku=BK-221
ok 2 - cart c-1001 holds two items
# ts=2026-07-01T08:10:04Z level=info msg="applying coupon" cart_id=c-1001 code=SPRING10
# ts=2026-07-01T08:10:05Z level=info msg="coupon applied" cart_id=c-1001 discount=4.31
ok 3 - coupon reduces total for c-1001
# ts=2026-07-01T08:10:06Z level=info msg="payment authorized" amount=38.79 cart_id=c-1001 provider=stripe
# ts=2026-07-01T08:10:07Z level=info msg="order persisted" cart_id=c-1001 order_id=o-5521
ok 4 - order for c-1001 is persisted
# ts=2026-07-01T08:10:08Z level=info msg="cart created" cart_id=c-1002 user=bob
# ts=2026-07-01T08:10:09Z level=debug msg="added item" cart_id=c-1002 qty=2 sku=BK-221
ok 5 - cart c-1002 holds two items
# ts=2026-07-01T08:10:10Z level=info msg="applying coupon" cart_id=c-1002 code=SPRING10
# ts=2026-07-01T08:10:11Z level=warn msg="coupon service slow" cart_id=c-1002 duration_ms=2350
# ts=2026-07-01T08:10:12Z level=error msg="coupon lookup failed" cart_id=c-1002 code=SPRING10 error=timeout
not ok 6 - coupon reduces total for c-1002

#   Failed test 'coupon reduces total for c-1002'
#   at t/checkout.t line 29.
#          got: '43.1'
#     expected: '38.79'
# ts=2026-07-01T08:10:13Z level=info msg="payment authorized" amount=38.79 cart_id=c-1002 provider=stripe
# ts=2026-07-01T08:10:14Z level=info msg="order persisted" cart_id=c-1002 order_id=o-5522
ok 7 - order for c-1002 is persisted
# ts=2026-07-01T08:10:15Z level=info msg="cart created" cart_id=c-1003 user=carol
# ts=2026-07-01T08:10:16Z level=debug msg="added item" cart_id=c-1003 qty=2 sku=BK-221
ok 8 - cart c-1003 holds two items
# ts=2026-07-01T08:10:17Z level=info msg="applying coupon" cart_id=c-1003 code=SPRING10
# ts=2026-07-01T08:10:18Z level=info msg="coupon applied" cart_id=c-1003 discount=4.31
ok 9 - coupon reduces total for c-1003
# ts=2026-07-01T08:10:19Z level=info msg="payment authorized" amount=38.79 cart_id=c-1003 provider=stripe
# ts=2026-07-01T08:10:20Z level=error msg="order not persisted" cart_id=c-1003 error="deadlock detected"
not ok 10 - order for c-1003 is persisted

#   Failed test 'order for c-1003 is persisted'
#   at t/checkout.t line 38.
# rolling back transaction for cart c-1003
# ts=2026-07-01T08:10:21Z level=warn msg="refund endpoint missing" cart_id=c-1001
not ok 11 - refund restores stock # TODO refunds not implemented
#   Failed (TODO) test 'refund restores stock'
#   at t/checkout.t line 49.
# ts=2026-07-01T08:10:22Z level=info msg="stopping test server"
# Looks like you failed 2 tests of 11.
Dubious, test returned 2 (wstat 512, 0x200)
Failed 2/11 subtests 

Test Summary Report
-------------------
t/checkout.t (Wstat: 512 Tests: 11 Failed: 2)
  Failed tests:  6, 10
  Non-zero exit status: 2
Files=1, Tests=11,  0 wallclock secs ( 0.02 usr  0.00 sys +  0.05 cusr  0.01 csys =  0.08 CPU)
Result: FAIL
