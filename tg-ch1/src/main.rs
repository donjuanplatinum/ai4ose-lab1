//! # Chapter 1: 应用程序与基本执行环境 (图形版 - 最终修复)
//!
//! 本章实现了一个基于 VirtIO GPU 的裸机图形程序。
//!
//! ## 关键修复点
//!
//! 1. **DMA 对齐**：使用 `#[repr(align(4096))]` 强制 DMA 堆按页对齐，防止 VirtIO 驱动 Panic。
//! 2. **借用检查**：先获取分辨率，释放 GPU 借用后，再获取 Framebuffer。
//! 3. **安全性**：使用 `addr_of_mut!` 访问 `static mut`，符合 Rust 2024 标准。
//! 4. **API 适配**：适配 `virtio-drivers` v0.12.0 的 Transport 抽象层。

#![no_std]
#![no_main]
#![cfg_attr(target_arch = "riscv64", deny(warnings, missing_docs))]
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]

extern crate alloc;

use core::ptr::{self, NonNull, addr_of_mut};
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use tg_sbi::{console_putchar, shutdown};

use virtio_drivers::{
    device::gpu::VirtIOGpu,
    transport::{mmio::MmioTransport, DeviceType, Transport},
    BufferDirection, Hal, PhysAddr,
};

// ==========================================
// 1. 全局内存分配器 (Bump Allocator)
// ==========================================

/// 一个简易的线性分配器。
/// virtio-drivers 内部需要 Vec 等集合类型，因此必须实现全局分配器。
struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: UnsafeCell<usize>,
}

unsafe impl Sync for BumpAllocator {}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let head = self.next.get();
        let start = unsafe { *head };
        
        let align_offset = start % layout.align();
        let padding = if align_offset == 0 { 0 } else { layout.align() - align_offset };
        let alloc_start = start + padding;
        let alloc_end = alloc_start + layout.size();

        if alloc_end > self.heap_end {
            ptr::null_mut()
        } else {
            unsafe { *head = alloc_end; }
            alloc_start as *mut u8
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // 本分配器极简，不支持内存回收
    }
}

const HEAP_SIZE: usize = 1024 * 1024; // 1MB 堆空间
static mut HEAP_SPACE: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator {
    heap_start: 0,
    heap_end: 0,
    next: UnsafeCell::new(0),
};

// ==========================================
// 2. HAL 实现 (硬件抽象层)
// ==========================================

/// VirtIO HAL 实现。
struct VirtioHal;

const DMA_HEAP_SIZE: usize = 512 * 4096; // 2MB DMA 空间

/// 包装结构体以强制 4K 对齐。
/// VirtIO 协议要求 DMA 地址必须页对齐，否则会导致 Panic。
#[repr(align(4096))]
struct AlignedDma([u8; DMA_HEAP_SIZE]);

static mut DMA_HEAP: AlignedDma = AlignedDma([0; DMA_HEAP_SIZE]);
static mut DMA_HEAD: usize = 0;

unsafe impl Hal for VirtioHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let size = pages * 4096;
        unsafe {
            if DMA_HEAD + size > DMA_HEAP_SIZE {
                panic!("DMA OOM");
            }
            let offset = DMA_HEAD;
            DMA_HEAD += size;

            // 使用 addr_of_mut! 获取裸指针，避免对 static mut 创建引用 (UB)
            let base_ptr = addr_of_mut!(DMA_HEAP.0) as *mut u8;
            let ptr = base_ptr.add(offset);
            
            // 清零内存
            core::ptr::write_bytes(ptr, 0, size);
            
            let paddr = ptr as usize as u64; 
            (paddr, NonNull::new_unchecked(ptr))
        }
    }

    unsafe fn dma_dealloc(_paddr: PhysAddr, _vaddr: NonNull<u8>, _pages: usize) -> i32 {
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        unsafe { NonNull::new_unchecked(paddr as usize as *mut u8) }
    }

    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        let vaddr = buffer.as_ptr() as *mut u8 as usize;
        vaddr as u64 
    }

    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {
    }
}

// ==========================================
// 3. 绘图与主逻辑
// ==========================================

/// 简单的颜色结构体。
#[derive(Clone, Copy)]
struct Color { r: u8, g: u8, b: u8, a: u8 }

impl Color {
    const WHITE: Color  = Color { r: 255, g: 255, b: 255, a: 255 };
    const RED: Color    = Color { r: 255, g: 0,   b: 0,   a: 255 };
    const BLUE: Color   = Color { r: 0,   g: 0,   b: 255, a: 255 };
    
    fn to_u32(&self) -> u32 {
        u32::from_le_bytes([self.r, self.g, self.b, self.a])
    }
}

fn draw_rect(fb: &mut [u8], stride: usize, x: usize, y: usize, w: usize, h: usize, color: Color) {
    let fb_u32 = unsafe {
        core::slice::from_raw_parts_mut(fb.as_mut_ptr() as *mut u32, fb.len() / 4)
    };
    let pixel = color.to_u32();
    let width = stride / 4; 

    for row in y..(y + h) {
        for col in x..(x + w) {
            let idx = row * width + col;
            if idx < fb_u32.len() {
                fb_u32[idx] = pixel;
            }
        }
    }
}

/// S 态程序入口点。
#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
unsafe extern "C" fn _start() -> ! {
    const STACK_SIZE: usize = 16 * 4096;
    #[unsafe(link_section = ".bss.uninit")]
    static mut STACK: [u8; STACK_SIZE] = [0u8; STACK_SIZE];

    core::arch::naked_asm!(
        "la sp, {stack} + {stack_size}",
        "j  {main}",
        stack_size = const STACK_SIZE,
        stack      =   sym STACK,
        main       =   sym rust_main,
    )
}

/// Rust 主逻辑入口。
extern "C" fn rust_main() -> ! {
    // 0. 初始化堆分配器
    unsafe {
        let heap_ptr = addr_of_mut!(HEAP_SPACE) as *mut u8 as usize;
        let allocator = &ALLOCATOR as *const BumpAllocator as *mut BumpAllocator;
        (*allocator).heap_start = heap_ptr;
        (*allocator).heap_end = heap_ptr + HEAP_SIZE;
        *(*allocator).next.get() = heap_ptr;
    }

    print_str("[Kernel] Start (Final Version)...\n");

    let mut gpu_device: Option<VirtIOGpu<VirtioHal, MmioTransport>> = None;

    // 1. 扫描 MMIO 总线寻找 GPU
    for i in 0..8 {
        let addr = 0x10001000 + i * 0x1000;
        let header_ptr = NonNull::new(addr as *mut _).unwrap();
        
        if let Ok(transport) = unsafe { MmioTransport::new(header_ptr, 0x1000) } {
             if transport.device_type() == DeviceType::GPU {
                print_str("[Kernel] Found GPU.\n");
                match VirtIOGpu::<VirtioHal, MmioTransport>::new(transport) {
                    Ok(gpu) => {
                        gpu_device = Some(gpu);
                        break;
                    },
                    Err(_) => print_str("[Kernel] GPU init failed\n"),
                }
             }
        }
    }

    if let Some(mut gpu) = gpu_device {
        // 2. 先获取分辨率 (避免与 setup_framebuffer 的借用冲突)
        let (width, height) = match gpu.resolution() {
            Ok(r) => r,
            Err(_) => {
                print_str("[Kernel] Warn: Failed to get resolution, using default.\n");
                (800, 600)
            }
        };
        
        let w = width as usize;
        let h = height as usize;

        // 3. 设置 Framebuffer
        match gpu.setup_framebuffer() {
            Ok(fb) => {
                print_str("[Kernel] Painting...\n");
                
                // 清屏
                draw_rect(fb, w * 4, 0, 0, w, h, Color::WHITE);
                
                // 绘制七巧板 OS 图案
                let start_x = 100;
                let start_y = 100;
                
                // O - 红色部分
                draw_rect(fb, w * 4, start_x, start_y, 50, 200, Color::RED);   
                draw_rect(fb, w * 4, start_x, start_y, 150, 50, Color::RED);   
                draw_rect(fb, w * 4, start_x, start_y + 150, 150, 50, Color::RED); 
                draw_rect(fb, w * 4, start_x + 100, start_y, 50, 200, Color::RED); 

                // S - 蓝色部分
                let sx = 300;
                draw_rect(fb, w * 4, sx, start_y, 150, 50, Color::BLUE);      
                draw_rect(fb, w * 4, sx, start_y, 50, 100, Color::BLUE);      
                draw_rect(fb, w * 4, sx, start_y + 75, 150, 50, Color::BLUE);      
                draw_rect(fb, w * 4, sx + 100, start_y + 75, 50, 125, Color::BLUE); 
                draw_rect(fb, w * 4, sx, start_y + 150, 150, 50, Color::BLUE);      

                // 4. 刷新缓冲区到屏幕
                let _ = gpu.flush();
                print_str("[Kernel] Done.\n");
            },
            Err(_) => print_str("[Kernel] FB setup failed\n"),
        }
        
        loop {}
    } else {
        print_str("[Kernel] No GPU found.\n");
    }

    shutdown(false)
}

fn print_str(s: &str) {
    for c in s.bytes() { console_putchar(c); }
}

/// Panic 处理函数。
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    print_str("Panic: ");
    if let Some(_loc) = info.location() {
        print_str(" at location\n");
    }
    shutdown(true)
}

/// 非 RISC-V 架构存根。
#[cfg(not(target_arch = "riscv64"))]
mod stub {
    /// 存根入口。
    #[unsafe(no_mangle)] pub extern "C" fn main() -> i32 { 0 }
    /// Libc 启动存根。
    #[unsafe(no_mangle)] pub extern "C" fn __libc_start_main() -> i32 { 0 }
    /// Rust 异常存根。
    #[unsafe(no_mangle)] pub extern "C" fn rust_eh_personality() {}
}
