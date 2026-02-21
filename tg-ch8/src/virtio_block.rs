//! VirtIO 块设备驱动模块
//!
//! 通过 MMIO 方式访问 QEMU virt 平台的 VirtIO 块设备，
//! 实现 `BlockDevice` trait 以供 easy-fs 使用。
//!
//! 已升级到 virtio-drivers 0.7.3 API 以支持 VirtIO-GPU/Input。

use crate::Sv39;
use alloc::sync::Arc;
use core::ptr::NonNull;
use spin::{Lazy, Mutex};
use tg_easy_fs::BlockDevice;
use tg_kernel_vm::page_table::MmuMeta;
use virtio_drivers::{
    device::blk::VirtIOBlk,
    transport::{mmio::MmioTransport, Transport},
    BufferDirection, Hal, PhysAddr,
};

/// VirtIO 设备 MMIO 基地址
const VIRTIO0: usize = 0x10001000;

/// 全局块设备实例（延迟初始化）
pub static BLOCK_DEVICE: Lazy<Arc<dyn BlockDevice>> = Lazy::new(|| {
    println!("[DEBUG] BLOCK_DEVICE: Lazy initialization starting...");
    let transport = unsafe {
        MmioTransport::new(NonNull::new(VIRTIO0 as *mut ()).unwrap().cast())
            .expect("Error when creating MmioTransport")
    };
    println!("[DEBUG] BLOCK_DEVICE: Transport created, type: {:?}", transport.device_type());
    let blk = VirtIOBlk::new(transport).expect("Error when creating VirtIOBlk");
    println!("[DEBUG] BLOCK_DEVICE: VirtIOBlk instance created!");
    Arc::new(VirtIOBlock(Mutex::new(blk)))
});
/// VirtIO 块设备封装
struct VirtIOBlock(Mutex<VirtIOBlk<VirtioHal, MmioTransport>>);

// Safety: 内部使用 Mutex 保护，确保线程安全
unsafe impl Send for VirtIOBlock {}
unsafe impl Sync for VirtIOBlock {}

impl BlockDevice for VirtIOBlock {
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        self.0.lock().read_blocks(block_id, buf)
            .expect("Error when reading VirtIOBlk");
    }
    fn write_block(&self, block_id: usize, buf: &[u8]) {
        self.0.lock().write_blocks(block_id, buf)
            .expect("Error when writing VirtIOBlk");
    }
}

/// VirtIO HAL（硬件抽象层）实现 — virtio-drivers 0.7.3 API
pub struct VirtioHal;

use core::sync::atomic::{AtomicUsize, Ordering};

#[repr(C, align(4096))]
struct DmaPool([u8; 8 * 1024 * 1024]);
#[unsafe(link_section = ".data")]
static mut DMA_POOL: DmaPool = DmaPool([1u8; 8 * 1024 * 1024]);
static DMA_OFFSET: AtomicUsize = AtomicUsize::new(0);

unsafe impl Hal for VirtioHal {
    /// DMA 内存分配 (使用静态池避开 buddy 分配器 panic)
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let size = pages << Sv39::PAGE_BITS;
        let offset = DMA_OFFSET.fetch_add(size, Ordering::SeqCst);
        if offset + size > 8 * 1024 * 1024 {
            panic!("DMA_POOL exhausted! requested {} bytes", size);
        }
        let ptr = unsafe { (DMA_POOL.0.as_mut_ptr() as usize + offset) as *mut u8 };
        // Manual zeroing
        unsafe { core::ptr::write_bytes(ptr, 0, size) };
        
        println!("[DMA] static_alloc: pages={}, size={}, ptr={:p}", pages, size, ptr);
        let paddr = ptr as usize;
        (paddr, NonNull::new(ptr).unwrap())
    }

    /// DMA 内存释放 (静态池不实际释放)
    unsafe fn dma_dealloc(_paddr: PhysAddr, vaddr: NonNull<u8>, pages: usize) -> i32 {
        println!("[DMA] static_dealloc (no-op): ptr={:p}, pages={}", vaddr.as_ptr(), pages);
        0
    }

    /// 物理地址转虚拟地址（恒等映射）
    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        if paddr == 0 {
            panic!("mmio_phys_to_virt: paddr is 0!");
        }
        unsafe { NonNull::new_unchecked(paddr as *mut u8) }
    }

    /// Share buffer for DMA
    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        buffer.as_ptr() as *mut u8 as usize
    }

    /// Unshare buffer
    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {}
}
