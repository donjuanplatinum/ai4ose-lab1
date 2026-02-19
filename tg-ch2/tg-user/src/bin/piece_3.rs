#![no_std]
#![no_main]

extern crate user_lib;

use user_lib::sys_draw_piece;

#[no_mangle]
fn main() -> i32 {
    sys_draw_piece(3);
    0
}
