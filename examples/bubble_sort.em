fn bubble_sort(xs: [int]) -> [int] {
    let mut result = xs;
    let mut i = 0;
    while i < result.length() {
        let mut j = 0;
        while j < result.length() - 1 - i {
            if result[j] > result[j + 1] {
                let tmp = result[j];
                result[j] = result[j + 1];
                result[j + 1] = tmp;
            }
            j = j + 1;
        }
        i = i + 1;
    }
    result
}

fn main() -> () {
    let sorted = bubble_sort([5, 3, 8, 1, 9, 2]);
    let mut i = 0;
    while i < sorted.length() {
        println(to_string(sorted[i]));
        i = i + 1;
    }
}
