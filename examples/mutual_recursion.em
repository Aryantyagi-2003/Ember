fn main() -> () {
    fn is_even(n: int) -> bool {
        if n == 0 { true } else { is_odd(n - 1) }
    }
    fn is_odd(n: int) -> bool {
        if n == 0 { false } else { is_even(n - 1) }
    }

    println(to_string(is_even(10))); // true
}
