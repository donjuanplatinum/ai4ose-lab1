#![allow(dead_code)]
pub const PLIC_BASE: usize = 0x0c00_0000;
pub const PLIC_PRIORITY_BASE: usize = PLIC_BASE;
pub const PLIC_PENDING_BASE: usize = PLIC_BASE + 0x1000;
pub const PLIC_SENABLE_BASE: usize = PLIC_BASE + 0x2080;
pub const PLIC_SPRIORITY_BASE: usize = PLIC_BASE + 0x20_1000;
pub const PLIC_SCLAIM_BASE: usize = PLIC_BASE + 0x20_1004;

pub fn init() {
    let _hart_id = 0;
    // Enable VirtIO Input interrupt (IRQ 11-18 depending on binding, assuming IRQ 1-8 for virtio here)
    for irq in 1..=8 {
        unsafe {
            // Set priority to 1
            let prio_ptr = (PLIC_PRIORITY_BASE + irq * 4) as *mut u32;
            core::ptr::write_volatile(prio_ptr, 1);
        }
    }
    unsafe {
        // Enable IRQs 1-8 for S-mode on Hart 0
        let enable_ptr = PLIC_SENABLE_BASE as *mut u32;
        core::ptr::write_volatile(enable_ptr, 0x01fe); // Bits 1-8
        
        // Set threshold to 0
        let threshold_ptr = PLIC_SPRIORITY_BASE as *mut u32;
        core::ptr::write_volatile(threshold_ptr, 0);
    }
}

pub fn claim() -> u32 {
    let claim_ptr = PLIC_SCLAIM_BASE as *mut u32;
    unsafe { core::ptr::read_volatile(claim_ptr) }
}

pub fn complete(irq: u32) {
    let claim_ptr = PLIC_SCLAIM_BASE as *mut u32;
    unsafe { core::ptr::write_volatile(claim_ptr, irq) }
}
