fn make_counter(start: int) -> fn() -> int {
    let mut count = start;
    fn() -> int {
        count = count + 1;
        count
    }
}

fn main() -> () {
    let counter_a = make_counter(0);
    let counter_b = make_counter(100);

    println(to_string(counter_a()));  // 1
    println(to_string(counter_a()));  // 2
    println(to_string(counter_b()));  // 101 -- independent from counter_a
    println(to_string(counter_a()));  // 3
}
