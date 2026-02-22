#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;
use user_lib::{open, read, close};

#[no_mangle]
pub fn main() -> i32 {
    let mut buf1 = [0u8; 16];
    let mut buf2 = [0u8; 16];
    
    println!("Testing random device /dev/random...");
    
    let fd = open("/dev/random", user_lib::OpenFlags::RDONLY);
    if fd < 0 {
        println!("Error: Could not open /dev/random");
        return -1;
    }
    
    if read(fd as usize, &mut buf1) <= 0 {
        println!("Error: Could not read from /dev/random");
        close(fd as usize);
        return -2;
    }
    
    println!("Read 1: {:?}", buf1);
    
    if read(fd as usize, &mut buf2) <= 0 {
        println!("Error: Could not read from /dev/random step 2");
        close(fd as usize);
        return -3;
    }
    
    println!("Read 2: {:?}", buf2);
    
    if buf1 == buf2 {
        println!("Error: Consecutive reads returned identical data (not very random!)");
        close(fd as usize);
        return -4;
    }
    
    // Check if it's all zeros
    let mut all_zeros = true;
    for &b in buf1.iter() {
        if b != 0 {
            all_zeros = false;
            break;
        }
    }
    if all_zeros {
        println!("Error: Random data is all zeros");
        close(fd as usize);
        return -5;
    }
    
    close(fd as usize);
    println!("Random test passed!");
    0
}
