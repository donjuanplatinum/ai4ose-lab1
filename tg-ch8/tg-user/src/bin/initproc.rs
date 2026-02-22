#![no_std]
#![no_main]

extern crate user_lib;

use user_lib::{exec, fork, wait};

#[no_mangle]
extern "C" fn main() -> i32 {
    if fork() == 0 {
        let target = "user_shell";
        exec(target);
    } else {
        loop {
            let mut exit_code: i32 = 0;
            let pid = wait(&mut exit_code);
            if pid == -1 {
                break;
            }
        }
    }
    0
}
