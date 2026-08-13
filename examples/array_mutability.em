fn sum(xs: [int]) -> int {
    fn go(xs: [int], i: int, acc: int) -> int {
        if i >= xs.length() {
            acc
        } else {
            go(xs, i + 1, acc + xs[i])
        }
    }
    go(xs, 0, 0)
}

fn main() -> () {
    let mut nums = [1, 2, 3];
    nums.push(4);
    nums.push(5);

    println(to_string(sum(nums))); // 15

    // let frozen = [1, 2, 3];
    // frozen.push(4); // <- type error: `frozen` is not declared `mut`
}
