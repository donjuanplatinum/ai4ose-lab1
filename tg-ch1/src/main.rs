#![no_std]
#![no_main]
#![cfg_attr(target_arch = "riscv64", deny(warnings))]
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

// --- 内存分配器 ---
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

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

const HEAP_SIZE: usize = 1024 * 1024; 
static mut HEAP_SPACE: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator {
    heap_start: 0,
    heap_end: 0,
    next: UnsafeCell::new(0),
};

// --- VirtIO HAL ---
struct VirtioHal;
const DMA_HEAP_SIZE: usize = 512 * 4096;

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
            let base_ptr = addr_of_mut!(DMA_HEAP.0) as *mut u8;
            let ptr = base_ptr.add(offset);
            core::ptr::write_bytes(ptr, 0, size);
            let paddr = ptr as usize as u64; 
            (paddr, NonNull::new_unchecked(ptr))
        }
    }
    unsafe fn dma_dealloc(_paddr: PhysAddr, _vaddr: NonNull<u8>, _pages: usize) -> i32 { 0 }
    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        unsafe { NonNull::new_unchecked(paddr as usize as *mut u8) }
    }
    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        let vaddr = buffer.as_ptr() as *mut u8 as usize;
        vaddr as u64 
    }
    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {}
}

// --- 图形绘制逻辑 ---

#[derive(Clone, Copy)]
struct Color { r: u8, g: u8, b: u8, a: u8 }

impl Color {
    const WHITE: Color      = Color { r: 255, g: 255, b: 255, a: 255 };
    
    // 对应 'OS' 图片的配色
    const DARK_BLUE: Color  = Color { r: 10,  g: 20,  b: 200, a: 255 }; // O左上角，S顶部
    const CYAN: Color       = Color { r: 10,  g: 210, b: 255, a: 255 }; // O左侧，S左上
    const YELLOW: Color     = Color { r: 245, g: 200, b: 10,  a: 255 }; // O底部
    const PINK: Color       = Color { r: 220, g: 80,  b: 240, a: 255 }; // O右上，S右侧
    const RED: Color        = Color { r: 240, g: 30,  b: 60,  a: 255 }; // O右中，S底部
    const LIME: Color       = Color { r: 40,  g: 240, b: 100, a: 255 }; // O右下，S中间
    const AZURE: Color      = Color { r: 0,   g: 140, b: 255, a: 255 }; // S腹部
    
    fn to_u32(&self) -> u32 {
        u32::from_le_bytes([self.r, self.g, self.b, self.a])
    }
}

fn set_pixel(fb_u32: &mut [u32], width: usize, height: usize, x: isize, y: isize, color: u32) {
    if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
        fb_u32[y as usize * width + x as usize] = color;
    }
}

/// 绘制填充三角形
fn draw_triangle(
    fb: &mut [u8], stride: usize, 
    p1: (isize, isize), p2: (isize, isize), p3: (isize, isize), 
    color: Color
) {
    let width = stride / 4;
    let height = fb.len() / stride; 
    let fb_u32 = unsafe {
        core::slice::from_raw_parts_mut(fb.as_mut_ptr() as *mut u32, fb.len() / 4)
    };
    let pixel_val = color.to_u32();

    let min_x = p1.0.min(p2.0).min(p3.0).max(0);
    let max_x = p1.0.max(p2.0).max(p3.0).min(width as isize - 1);
    let min_y = p1.1.min(p2.1).min(p3.1).max(0);
    let max_y = p1.1.max(p2.1).max(p3.1).min(height as isize - 1);

    let edge = |a: (isize, isize), b: (isize, isize), c: (isize, isize)| -> isize {
        (c.0 - a.0) * (b.1 - a.1) - (c.1 - a.1) * (b.0 - a.0)
    };

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = (x, y);
            let w0 = edge(p1, p2, p);
            let w1 = edge(p2, p3, p);
            let w2 = edge(p3, p1, p);
            if (w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0) {
                set_pixel(fb_u32, width, height, x, y, pixel_val);
            }
        }
    }
}

/// 绘制四边形 (p1->p2->p3->p4)
fn draw_quad(
    fb: &mut [u8], stride: usize,
    p1: (isize, isize), p2: (isize, isize), 
    p3: (isize, isize), p4: (isize, isize), 
    color: Color
) {
    draw_triangle(fb, stride, p1, p2, p3, color);
    draw_triangle(fb, stride, p1, p3, p4, color);
}

fn draw_fill_rect(fb: &mut [u8], stride: usize, x: usize, y: usize, w: usize, h: usize, color: Color) {
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

// --- 主程序 ---

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

extern "C" fn rust_main() -> ! {
    unsafe {
        let heap_ptr = addr_of_mut!(HEAP_SPACE) as *mut u8 as usize;
        let allocator = &ALLOCATOR as *const BumpAllocator as *mut BumpAllocator;
        (*allocator).heap_start = heap_ptr;
        (*allocator).heap_end = heap_ptr + HEAP_SIZE;
        *(*allocator).next.get() = heap_ptr;
    }

    print_str("[Kernel] Drawing 'OS' Tangram...\n");

    let mut gpu_device: Option<VirtIOGpu<VirtioHal, MmioTransport>> = None;

    for i in 0..8 {
        let addr = 0x10001000 + i * 0x1000;
        let header_ptr = NonNull::new(addr as *mut _).unwrap();
        if let Ok(transport) = unsafe { MmioTransport::new(header_ptr, 0x1000) } {
            if transport.device_type() == DeviceType::GPU {
                match VirtIOGpu::<VirtioHal, MmioTransport>::new(transport) {
                    Ok(gpu) => { gpu_device = Some(gpu); break; },
                    Err(_) => print_str("[Kernel] GPU init failed\n"),
                }
            }
        }
    }

    if let Some(mut gpu) = gpu_device {
        let (width, height) = match gpu.resolution() {
            Ok(r) => r,
            Err(_) => (800, 600),
        };
        let w = width as usize;
        let h = height as usize;

        match gpu.setup_framebuffer() {
            Ok(fb) => {
                // 白底
                draw_fill_rect(fb, w * 4, 0, 0, w, h, Color::WHITE);
                let stride = w * 4;
                
                // ==========================================
                //  字母 'O' (左侧)
                // ==========================================
                let o_x = 50; 
                let o_y = 50;

                // 1. 左上角 - 深蓝小三角形 (Dark Blue)
                draw_triangle(fb, stride, 
			      (o_x, o_y), (o_x + 100, o_y), (o_x, o_y + 100), 
			      Color::DARK_BLUE);

                // 2. 左侧 - 青色平行四边形 (Cyan)
                // 连接深蓝下方
                draw_quad(fb, stride, 
			  (o_x, o_y + 100), (o_x + 100, o_y), 
			  (o_x + 100, o_y + 200), (o_x, o_y + 300), 
			  Color::CYAN);

                // 3. 底部 - 黄色大三角形 (Yellow)
                draw_triangle(fb, stride, 
			      (o_x, o_y + 300), (o_x + 200, o_y + 500), (o_x, o_y + 500), 
			      Color::YELLOW);

                // 4. 右上角 - 粉色大三角形 (Pink)
                draw_triangle(fb, stride, 
			      (o_x + 100, o_y), (o_x + 300, o_y), (o_x + 300, o_y + 200), 
			      Color::PINK);

                // 5. 右中 - 红色平行四边形 (Red)
                // (250, 150) -> (350, 250) -> (350, 450) -> (250, 350)
                draw_quad(fb, stride,
			  (o_x + 200, o_y + 100), (o_x + 300, o_y + 200),
			  (o_x + 300, o_y + 400), (o_x + 200, o_y + 300),
			  Color::RED);

                // 6. 右下 - 绿色菱形/正方形 (Lime)
                // 填补红色和黄色之间的空缺
                draw_quad(fb, stride,
			  (o_x + 100, o_y + 400), // 左
			  (o_x + 200, o_y + 300), // 上
			  (o_x + 300, o_y + 400), // 右
			  (o_x + 200, o_y + 500), // 下
			  Color::LIME);


                // ==========================================
                //  字母 'S' (右侧)
                // ==========================================
                let s_x = 450;
		let s_y = 50;
		let stride = w * 4;

		// 1. 顶部小三角形 (Red) - 构成顶部横梁的左侧部分
		draw_triangle(fb, stride, 
			      (s_x + 150, s_y), 
			      (s_x + 250, s_y), 
			      (s_x + 250, s_y + 100), 
			      Color::RED);

		// 2. 顶部大三角形 (Pink) - 构成顶部横梁的右侧及尖端
		draw_triangle(fb, stride, 
			      (s_x + 250, s_y - 20), 
			      (s_x + 450, s_y - 20), 
			      (s_x + 250, s_y +100 ), 
			      Color::PINK);

		// 3. 左侧中三角形 (Yellow) - 构成 S 中间的左侧凸起
		draw_triangle(fb, stride, 
			      (s_x + 150, s_y), 
			      (s_x + 50,  s_y + 100), 
			      (s_x + 150, s_y + 200), 
			      Color::YELLOW);

		// 4. 中间正方形 (Lime) - S 的核心连接处
		// 放置在 (150, 200) 位置，大小 100x100
		draw_fill_rect(fb, stride, (s_x + 150) as usize, (s_y + 200) as usize, 100, 100, Color::LIME);

		// 5. 右侧大三角形 (Pink) - 构成 S 下半部分的右侧凸起
		draw_triangle(fb, stride, 
			      (s_x + 250, s_y + 200), 
			      (s_x + 350, s_y + 300), 
			      (s_x + 250, s_y + 400), 
			      Color::PINK);

		// 6. 底部平行四边形 (Azure) - 构成底部的左侧尾巴
		// 坐标点：左上，右上，右下，左下
		draw_quad(fb, stride,
			  (s_x + 50,  s_y + 400), 
			  (s_x + 150, s_y + 400), 
			  (s_x + 200, s_y + 500), 
			  (s_x + 100, s_y + 500), 
			  Color::AZURE);

		// 7. 底部小三角形 (Red) - 填充底部尾巴的空隙
		draw_triangle(fb, stride, 
			      (s_x + 150, s_y + 400), 
			      (s_x + 250, s_y + 400), 
			      (s_x + 200, s_y + 500), 
			      Color::RED);

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

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    print_str("Panic: ");
    if let Some(_loc) = info.location() {
        print_str(" at location\n");
    }
    shutdown(true)
}
