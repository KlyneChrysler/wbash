use calc::*;

#[test]
fn arithmetic() {
    assert_eq!(old_name(2, 3), 5);
    assert_eq!(sub(9, 4), 5);
    assert_eq!(mul(3, 4), 12);
}

#[test]
fn helpers() {
    assert_eq!(clamp(12, 0, 10), 10);
    assert_eq!(parse_num(" 42 ").unwrap(), 42);
    assert_eq!(fib(10), 55);
}
