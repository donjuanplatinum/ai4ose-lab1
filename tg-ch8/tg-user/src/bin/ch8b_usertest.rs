#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

const TESTS: &[&str] = &[
    "00hello_world",
    "05write_a",
    "06write_b",
    "07write_c",
    "08power_3",
    "09power_5",
    "10power_7",
    "fork_exit",
    "forktest_simple",
    "12forktest",
    "filetest_simple",
    "cat_filea",
    "pipetest",
    "mpsc_sem",
    "phil_din_mutex",
    "race_adder_mutex_blocking",
    "sync_sem",
    "test_condvar",
    "threads",
    "threads_arg",
];

const TEST_NUM: usize = TESTS.len();

use user_lib::{exec, fork, waitpid};

#[no_mangle]
extern "C" fn main() -> i32 {
    for &test in TESTS {
        println!("Usertests: Running {}", test);
        let pid = fork();
        if pid == 0 {
            exec(test);
            panic!("unreachable!");
        } else {
            let mut xstate: i32 = Default::default();
            let wait_pid = waitpid(pid, &mut xstate);
            assert_eq!(pid, wait_pid);
            println!(
                "\x1b[32mUsertests: Test {} in Process {} exited with code {}\x1b[0m",
                test, pid, xstate
            );
        }
    }
    println!("Basic usertests passed!");
    0
}
