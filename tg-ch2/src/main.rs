//! # 第二章：批处理系统 —— 七巧板动态拼图 (User Mode Version)
//!
//! 内核初始化 VirtIO-GPU，但绘图指令由用户程序通过系统调用发出。
//! 内核按顺序运行 "piece_0" 到 "piece_13" 用户程序，每个程序绘制一块七巧板。
//!
//! ## 核心功能
//!
//! - **VirtIO-GPU 初始化**：内核完成
//! - **系统调用 `sys_draw_piece`**：用户程序请求绘制指定 ID 的七巧板
//! - **批处理循环**：运行用户程序 -> 延迟 500ms -> 运行下一个

#![no_std]
#![no_main]
#![cfg_attr(target_arch = "riscv64", deny(warnings))]
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code, unused_imports))]

extern crate alloc;

#[macro_use]
extern crate tg_console;

use impls::{Console, SyscallContext};
use riscv::register::*;
use tg_console::log;
use tg_kernel_context::LocalContext;
use tg_sbi;
use tg_syscall::{Caller, SyscallId};

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::{self, NonNull, addr_of_mut};

use virtio_drivers::{
    device::gpu::VirtIOGpu,
    transport::{mmio::MmioTransport, DeviceType, Transport},
    BufferDirection, Hal, PhysAddr,
};



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



#[derive(Clone, Copy)]
struct Color { r: u8, g: u8, b: u8, a: u8 }

impl Color {
    const WHITE: Color = Color { r: 255, g: 255, b: 255, a: 255 };
    const DARK_BLUE: Color = Color { r: 10, g: 20, b: 200, a: 255 };
    const CYAN: Color = Color { r: 10, g: 210, b: 255, a: 255 };
    const YELLOW: Color = Color { r: 245, g: 200, b: 10, a: 255 };
    const PINK: Color = Color { r: 220, g: 80, b: 240, a: 255 };
    const RED: Color = Color { r: 240, g: 30, b: 60, a: 255 };
    const LIME: Color = Color { r: 40, g: 240, b: 100, a: 255 };
    const AZURE: Color = Color { r: 0, g: 140, b: 255, a: 255 };

    fn to_u32(&self) -> u32 {
        u32::from_le_bytes([self.r, self.g, self.b, self.a])
    }
}

fn set_pixel(fb_u32: &mut [u32], width: usize, height: usize, x: isize, y: isize, color: u32) {
    if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
        fb_u32[y as usize * width + x as usize] = color;
    }
}

fn draw_triangle(fb: &mut [u8], stride: usize, p1: (isize, isize), p2: (isize, isize), p3: (isize, isize), color: Color) {
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

    let mut drawn_pixels = 0;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = (x, y);
            let w0 = edge(p1, p2, p);
            let w1 = edge(p2, p3, p);
            let w2 = edge(p3, p1, p);
            if (w0 >= 0 && w1 >= 0 && w2 >= 0) || (w0 <= 0 && w1 <= 0 && w2 <= 0) {
                set_pixel(fb_u32, width, height, x, y, pixel_val);
                drawn_pixels += 1;
            }
        }
    }
    if drawn_pixels > 0 {
        // println!("[Kernel] draw_triangle: drawn {} pixels", drawn_pixels);
    } else {
        println!("[Kernel] draw_triangle: WARNING 0 pixels drawn!");
    }
}

fn draw_quad(fb: &mut [u8], stride: usize, p1: (isize, isize), p2: (isize, isize), p3: (isize, isize), p4: (isize, isize), color: Color) {
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



fn render_piece(fb: &mut [u8], stride: usize, piece_id: usize) {
    let o_x: isize = 50;
    let o_y: isize = 50;
    let s_x: isize = 450;
    let s_y: isize = 50;

    match piece_id {
        0 => { 
            let width = stride / 4;
            let height = fb.len() / stride;
            draw_fill_rect(fb, stride, 0, 0, width, height, Color::WHITE);
        }
        1 => draw_triangle(fb, stride, (o_x, o_y), (o_x + 100, o_y), (o_x, o_y + 100), Color::DARK_BLUE),
        2 => draw_quad(fb, stride, (o_x, o_y + 100), (o_x + 100, o_y), (o_x + 100, o_y + 200), (o_x, o_y + 300), Color::CYAN),
        3 => draw_triangle(fb, stride, (o_x, o_y + 300), (o_x + 200, o_y + 500), (o_x, o_y + 500), Color::YELLOW),
        4 => draw_triangle(fb, stride, (o_x + 100, o_y), (o_x + 300, o_y), (o_x + 300, o_y + 200), Color::PINK),
        5 => draw_quad(fb, stride, (o_x + 200, o_y + 100), (o_x + 300, o_y + 200), (o_x + 300, o_y + 400), (o_x + 200, o_y + 300), Color::RED),
        6 => draw_quad(fb, stride, (o_x + 100, o_y + 400), (o_x + 200, o_y + 300), (o_x + 300, o_y + 400), (o_x + 200, o_y + 500), Color::LIME),
        7 => draw_triangle(fb, stride, (s_x + 150, s_y), (s_x + 250, s_y), (s_x + 250, s_y + 100), Color::RED),
        8 => draw_triangle(fb, stride, (s_x + 250, s_y - 20), (s_x + 450, s_y - 20), (s_x + 250, s_y + 100), Color::PINK),
        9 => draw_triangle(fb, stride, (s_x + 150, s_y), (s_x + 50, s_y + 100), (s_x + 150, s_y + 200), Color::YELLOW),
        10 => draw_fill_rect(fb, stride, (s_x + 150) as usize, (s_y + 200) as usize, 100, 100, Color::LIME),
        11 => draw_triangle(fb, stride, (s_x + 250, s_y + 200), (s_x + 350, s_y + 300), (s_x + 250, s_y + 400), Color::PINK),
        12 => draw_quad(fb, stride, (s_x + 50, s_y + 400), (s_x + 150, s_y + 400), (s_x + 200, s_y + 500), (s_x + 100, s_y + 500), Color::AZURE),
        13 => draw_triangle(fb, stride, (s_x + 150, s_y + 400), (s_x + 250, s_y + 400), (s_x + 200, s_y + 500), Color::RED),
        _ => {}
    }
}



fn sbi_delay_ms(ms: u64) {
    let ticks = ms * 10_000;
    let start: u64;
    unsafe { core::arch::asm!("rdtime {}", out(reg) start); }
    loop {
        let now: u64;
        unsafe { core::arch::asm!("rdtime {}", out(reg) now); }
        if now - start >= ticks { break; }
    }
}



static mut GPU_CONTEXT: Option<VirtIOGpu<VirtioHal, MmioTransport>> = None;
static mut FB_PTR: *mut u8 = ptr::null_mut();
static mut FB_LEN: usize = 0;
static mut FB_WIDTH: usize = 0;



#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(include_str!(env!("APP_ASM")));

#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
unsafe extern "C" fn _start() -> ! {
    const STACK_SIZE: usize = 16 * 4096;
    #[unsafe(link_section = ".boot.stack")]
    static mut STACK: [u8; STACK_SIZE] = [0u8; STACK_SIZE];

    core::arch::naked_asm!(
        "la sp, {stack} + {stack_size}",
        "j  {main}",
        stack = sym STACK,
        stack_size = const STACK_SIZE,
        main = sym rust_main,
    )
}



extern "C" fn rust_main() -> ! {
    unsafe { tg_linker::KernelLayout::locate().zero_bss() };
    tg_console::init_console(&Console);
    tg_console::set_log_level(option_env!("LOG"));
    tg_console::test_log();

    unsafe {
        let heap_ptr = addr_of_mut!(HEAP_SPACE) as *mut u8 as usize;
        let allocator = &ALLOCATOR as *const BumpAllocator as *mut BumpAllocator;
        (*allocator).heap_start = heap_ptr;
        (*allocator).heap_end = heap_ptr + HEAP_SIZE;
        *(*allocator).next.get() = heap_ptr;
    }

    tg_syscall::init_io(&SyscallContext);
    tg_syscall::init_process(&SyscallContext);


    println!("[Kernel] Initializing VirtIO-GPU...");
    for i in 0..8 {
        let addr = 0x10001000 + i * 0x1000;
        let header_ptr = NonNull::new(addr as *mut _).unwrap();
        if let Ok(transport) = unsafe { MmioTransport::new(header_ptr, 0x1000) } {
            if transport.device_type() == DeviceType::GPU {
                match VirtIOGpu::<VirtioHal, MmioTransport>::new(transport) {
                    Ok(mut gpu) => {
                        println!("[Kernel] VirtIO-GPU device declared");
                        match gpu.setup_framebuffer() {
                            Ok(fb) => {
                                unsafe {
                                    FB_PTR = fb.as_mut_ptr();
                                    FB_LEN = fb.len();
                                    FB_WIDTH = gpu.resolution().unwrap().0 as usize;
                                    GPU_CONTEXT = Some(gpu);
                                }
                                println!("[Kernel] Framebuffer initialized");
                            }
                            Err(_) => println!("[Kernel] Framebuffer setup failed"),
                        }
                        break;
                    }
                    Err(_) => println!("[Kernel] GPU init failed"),
                }
            }
        }
    }


    println!();
    println!("[Kernel] === Running user applications (Piecewise Rendering) ===");

    println!("[Kernel] Getting AppMeta manually...");
    unsafe extern "C" { fn apps(); }
    let apps_ptr = apps as *const usize;
    println!("[Kernel] apps_ptr: {:p}", apps_ptr);
    let app_count = unsafe { *apps_ptr.add(2) };
    println!("[Kernel] App count: {}", app_count);
    
    let app_ptrs = unsafe { core::slice::from_raw_parts(apps_ptr.add(3), app_count + 1) };

    const APP_BASE_ADDRESS: usize = 0x8080_0000;

    for i in 0..app_count {
        let app_src_start = app_ptrs[i];
        let app_src_end = app_ptrs[i+1];
        let app_len = app_src_end - app_src_start;
        
        println!("[Kernel] Loading app_{} from {:#x} to {:#x}, len={}", i, app_src_start, APP_BASE_ADDRESS, app_len);

    
        const MAX_APP_SIZE: usize = 0x40000; // 256KB
        let clear_region = unsafe { core::slice::from_raw_parts_mut(APP_BASE_ADDRESS as *mut u8, MAX_APP_SIZE) };
        clear_region.fill(0);
        
        let src = unsafe { core::slice::from_raw_parts(app_src_start as *const u8, app_len) };
        let dst = unsafe { core::slice::from_raw_parts_mut(APP_BASE_ADDRESS as *mut u8, app_len) };
        dst.copy_from_slice(src);
        
    
        unsafe { core::arch::asm!("fence.i"); }

    
        let mut ctx = LocalContext::user(APP_BASE_ADDRESS);
        let mut user_stack: core::mem::MaybeUninit<[usize; 512]> = core::mem::MaybeUninit::uninit();
        let user_stack_ptr = user_stack.as_mut_ptr() as *mut usize;
        *ctx.sp_mut() = unsafe { user_stack_ptr.add(512) } as usize;

    
        loop {
            unsafe { ctx.execute() };

            use scause::{Exception, Trap};
            match scause::read().cause() {
                Trap::Exception(Exception::UserEnvCall) => {
                    use SyscallResult::*;
                    match handle_syscall(&mut ctx) {
                        Done => continue,
                        Exit(code) => {
                            println!("[Kernel] App_{} exit with code {}", i, code);
    
                            if i > 0 {
                                sbi_delay_ms(500);
                            }
                        },
                        Error(id) => println!("[Kernel] App_{} call an unsupported syscall {}", i, id.0)
                    }
                }
                Trap::Exception(Exception::StoreFault) | Trap::Exception(Exception::StorePageFault) => {
                     println!("[Kernel] App_{} StoreFault addr={:#x}", i, stval::read());
                }
                Trap::Exception(Exception::LoadFault) | Trap::Exception(Exception::LoadPageFault) => {
                     println!("[Kernel] App_{} LoadFault addr={:#x}", i, stval::read());
                }
                trap => println!("[Kernel] App_{} was killed because of {:?}", i, trap),
            }
            unsafe { core::arch::asm!("fence.i") };
            break;
        }
        let _ = core::hint::black_box(&user_stack);
    }

    println!("[Kernel]All pieces drawn. System halting.");
    sbi_delay_ms(2000);
    tg_sbi::shutdown(false)
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("{info}");
    tg_sbi::shutdown(true)
}



enum SyscallResult {
    Done,
    Exit(usize),
    Error(SyscallId),
}

fn handle_syscall(ctx: &mut LocalContext) -> SyscallResult {
    use tg_syscall::{SyscallId as Id, SyscallResult as Ret};


    let id_raw = ctx.a(7);
    

    if id_raw == 500 {
        let piece_id = ctx.a(0);
        log::info!("[Syscall] request draw piece: {}", piece_id);

        unsafe {
            if let Some(gpu) = (*addr_of_mut!(GPU_CONTEXT)).as_mut() {
                if !FB_PTR.is_null() {
                    let fb = core::slice::from_raw_parts_mut(FB_PTR, FB_LEN);
                    let stride = FB_WIDTH * 4;
                    render_piece(fb, stride, piece_id);
                    match gpu.flush() {
                        Ok(_) => log::info!("[Kernel] gpu.flush() success"),
                        Err(e) => log::error!("[Kernel] gpu.flush() failed: {:?}", e),
                    }
                }
            }
        }
        
        *ctx.a_mut(0) = 0; // Return success
        move_next_insn(ctx);
        return SyscallResult::Done;
    }


    let id = id_raw.into();
    let args = [ctx.a(0), ctx.a(1), ctx.a(2), ctx.a(3), ctx.a(4), ctx.a(5)];

    match tg_syscall::handle(Caller { entity: 0, flow: 0 }, id, args) {
        Ret::Done(ret) => match id {
            Id::EXIT => SyscallResult::Exit(ctx.a(0)),
            _ => {
                *ctx.a_mut(0) = ret as _;
                move_next_insn(ctx);
                SyscallResult::Done
            }
        },
        Ret::Unsupported(id) => SyscallResult::Error(id),
    }
}

fn move_next_insn(ctx: &mut LocalContext) {

    let pc = ctx.pc();
    let insn: u16 = unsafe { core::ptr::read_unaligned(pc as *const u16) };
    let len: usize = if (insn & 0x3) == 0x3 { 4 } else { 2 };
    *ctx.pc_mut() = pc + len;
}



mod impls {
    use tg_syscall::{STDDEBUG, STDOUT};

    pub struct Console;

    impl tg_console::Console for Console {
        #[inline]
        fn put_char(&self, c: u8) {
            tg_sbi::console_putchar(c);
        }
    }

    pub struct SyscallContext;

    impl tg_syscall::IO for SyscallContext {
        fn write(&self, _caller: tg_syscall::Caller, fd: usize, buf: usize, count: usize) -> isize {
            match fd {
                STDOUT | STDDEBUG => {
                    print!("{}", unsafe {
                        core::str::from_utf8_unchecked(core::slice::from_raw_parts(buf as *const u8, count))
                    });
                    count as _
                }
                _ => -1
            }
        }
    }

    impl tg_syscall::Process for SyscallContext {
        #[inline]
        fn exit(&self, _caller: tg_syscall::Caller, _status: usize) -> isize {
            0
        }
    }
}

#[cfg(not(target_arch = "riscv64"))]
mod stub {
    #[unsafe(no_mangle)] pub extern "C" fn main() -> i32 { 0 }
    #[unsafe(no_mangle)] pub extern "C" fn __libc_start_main() -> i32 { 0 }
    #[unsafe(no_mangle)] pub extern "C" fn rust_eh_personality() {}
}
