mod a;
mod b;

fn main() {
    let x = 1;
    let y = 2;
    let z = add(x, y);
    println!("{}", z);
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}
