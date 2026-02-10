/// AI4OSE Lab1: 最簡單的 Rust 應用程式
///
/// 這是一個可發佈到 crates.io 的最小 Rust 應用程式範例，
/// 作為操作系統內核學習的起點。
fn main() {
    print!("{}", include_str!("content.txt"));
}

#[cfg(test)]
mod tests {
    #[test]
    fn AI4OSE_Lab1_2026S() {
        assert_eq!("ai4ose".to_string() + "-lab1" + "-2026s", "ai4ose-lab1-2026s");
    }
}
