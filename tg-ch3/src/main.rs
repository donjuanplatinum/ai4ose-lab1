//! # 第三章：多道程序与分时多任务
//!
//! 本章在第二章"批处理系统"的基础上，实现了一个**多道程序操作系统**，
//! 支持多个用户程序并发执行，并通过时钟中断实现抢占式调度。
//!
//! ## 核心概念
//!
//! - **多道程序**：多个用户程序同时驻留在内存中，内核在它们之间切换执行
//! - **任务控制块（TCB）**：管理每个任务的上下文、状态和资源
//! - **协作式调度**：任务通过 `yield` 系统调用主动让出 CPU
//! - **抢占式调度**：通过时钟中断强制切换任务，实现时间片轮转
//! - **系统调用**：`write`、`exit`、`yield`、`clock_gettime`
//!
//! 教程阅读建议：
//!
//! - 先看 `rust_main` 主循环：理解“轮转 + 时钟中断 + ecall”三类事件交织；
//! - 再看 `TaskControlBlock` 的使用方式：理解任务上下文与生命周期；
//! - 最后看 `impls::Clock`：理解硬件时钟到用户态时间结构体的桥接。

// 不使用标准库，裸机环境没有操作系统提供系统调用支持
#![no_std]
// 不使用标准入口，裸机环境没有 C runtime 进行初始化
#![no_main]
#![allow(static_mut_refs)]
// RISC-V64 架构下启用严格警告和文档检查
#![cfg_attr(target_arch = "riscv64", deny(warnings, missing_docs))]
// 非 RISC-V64 架构允许死代码（用于 cargo publish --dry-run 在主机上通过编译）
#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code))]

// 任务管理模块：定义任务控制块（TCB）和调度事件
mod task;
mod plic;

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::{self, NonNull, addr_of_mut};

use virtio_drivers::{
    device::gpu::VirtIOGpu,
    device::input::VirtIOInput,
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
            let paddr = ptr as usize;
            (paddr, NonNull::new_unchecked(ptr))
        }
    }
    unsafe fn dma_dealloc(_paddr: PhysAddr, _vaddr: NonNull<u8>, _pages: usize) -> i32 { 0 }
    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        unsafe { NonNull::new_unchecked(paddr as usize as *mut u8) }
    }
    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        let vaddr = buffer.as_ptr() as *mut u8 as usize;
        vaddr
    }
    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {}
}

static mut GPU_CONTEXT: Option<VirtIOGpu<VirtioHal, MmioTransport>> = None;
static mut INPUT_CONTEXT: Option<VirtIOInput<VirtioHal, MmioTransport>> = None;
static mut FB_PTR: *mut u8 = ptr::null_mut();
static mut FB_LEN: usize = 0;
static mut FB_WIDTH: usize = 0;


// 引入控制台输出宏（print! / println!），由 tg_console 库提供
#[macro_use]
extern crate tg_console;

// 本地模块：Console 和 SyscallContext 的实现
use impls::{Console, SyscallContext};
// riscv 库：访问 RISC-V 控制状态寄存器（CSR），如 scause、sie、time
use riscv::register::*;
// 任务控制块
use task::TaskControlBlock;
// 日志模块
use tg_console::log;
// SBI 调用：set_timer、console_putchar、shutdown 等
use tg_sbi;

// ========== 启动相关 ==========

// 将用户程序的二进制数据内联到内核镜像的 .data 段中
// APP_ASM 由 build.rs 在编译时生成，包含所有用户程序的二进制数据
#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(include_str!(env!("APP_ASM")));

// 最大支持的应用程序数量
const APP_CAPACITY: usize = 32;

// 定义内核入口点：分配 (APP_CAPACITY + 2) * 8 KiB = 272 KiB 的内核栈
// 比第二章更大，因为需要同时容纳多个任务的内核上下文。
//
// 这里不再调用 tg_linker::boot0! 宏，避免外部已发布版本与 Rust 2024
// 在属性语义上的兼容差异影响本 crate 的发布校验。
#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
unsafe extern "C" fn _start() -> ! {
    const STACK_SIZE: usize = (APP_CAPACITY + 32) * 8192;
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

// ========== 内核主函数 ==========

/// 内核主函数：初始化各子系统，然后以多道方式并发运行所有用户程序。
///
/// 与第二章的串行批处理不同，本章的多道程序系统支持：
/// - 多个任务同时驻留在内存中，每个任务拥有独立的 TCB 和用户栈
/// - 任务之间通过时间片轮转切换（抢占式调度，默认模式）
/// - 任务可以主动让出 CPU（协作式调度，通过 yield，需启用 `coop` feature）
extern "C" fn rust_main() -> ! {
    // 第一步：清零 BSS 段（未初始化的全局变量区域）
    unsafe { tg_linker::KernelLayout::locate().zero_bss() };

    // 第二步：初始化控制台输出（使 print!/println! 可用）
    // 默认日志级别为 info（可通过 LOG 环境变量覆盖）
    tg_console::init_console(&Console);
    tg_console::set_log_level(option_env!("LOG").or(Some("info")));
    tg_console::test_log();

    // 第三步：初始化系统调用处理
    // 比第二章多了 scheduling（yield）、clock（获取时间）和 trace（追踪，练习题）
    tg_syscall::init_io(&SyscallContext);
    tg_syscall::init_process(&SyscallContext);
    tg_syscall::init_scheduling(&SyscallContext);
    tg_syscall::init_clock(&SyscallContext);
    tg_syscall::init_trace(&SyscallContext);

    unsafe {
        let heap_ptr = addr_of_mut!(HEAP_SPACE) as *mut u8 as usize;
        let allocator = &ALLOCATOR as *const BumpAllocator as *mut BumpAllocator;
        (*allocator).heap_start = heap_ptr;
        (*allocator).heap_end = heap_ptr + HEAP_SIZE;
        *(*allocator).next.get() = heap_ptr;
    }

    log::info!("Initializing VirtIO devices...");
    for i in 0..8 {
        let addr = 0x10001000 + i * 0x1000;
        let header_ptr = NonNull::new(addr as *mut _).unwrap();
        if let Ok(transport) = unsafe { MmioTransport::new(header_ptr) } {
            match transport.device_type() {
                DeviceType::GPU => {
                    if let Ok(mut gpu) = VirtIOGpu::<VirtioHal, MmioTransport>::new(transport) {
                        log::info!("VirtIO-GPU device declared");
                        if let Ok(fb) = gpu.setup_framebuffer() {
                            unsafe {
                                FB_PTR = fb.as_mut_ptr();
                                FB_LEN = fb.len();
                                FB_WIDTH = gpu.resolution().unwrap().0 as usize;
                                GPU_CONTEXT = Some(gpu);
                            }
                            log::info!("Framebuffer initialized");
                        }
                    }
                }
                DeviceType::Input => {
                    log::info!("Found Input device");
                    if let Ok(mut input) = VirtIOInput::<VirtioHal, MmioTransport>::new(transport) {
                        log::info!("VirtIO-Input device declared");
                        input.ack_interrupt();
                        unsafe {
                            INPUT_CONTEXT = Some(input);
                        }
                    }
                }
                _ => {}
            }
        }
    }


    // 第四步：初始化任务控制块数组，加载所有用户程序
    log::info!("Preparing to initialize TCBs...");
    static mut TCBS: [TaskControlBlock; APP_CAPACITY] = [TaskControlBlock::ZERO; APP_CAPACITY];
    let tcbs = unsafe { &mut TCBS };
    let mut index_mod = 0;
    
    log::info!("Locating AppMeta...");
    for (i, app) in tg_linker::AppMeta::locate().iter().enumerate() {
        let entry = app.as_ptr() as usize;
        log::info!("load app{i} to {entry:#x}");
        tcbs[i].init(entry);
        index_mod += 1;
    }
    log::info!("All apps loaded");
    println!();

    // 第五步：开启 S 特权级时钟中断和外部中断
    // 这是实现抢占式调度的关键：允许时钟中断打断用户程序的执行
    // 外部中断用于接收 VirtIO 设备的输入事件
    log::info!("Enabling interrupts...");
    unsafe { 
        sie::set_stimer();
        sie::set_sext();
    }

    log::info!("Initializing PLIC...");
    plic::init();
    log::info!("PLIC initialized");

    // ========== 多道程序主循环 ==========
    // 使用轮转调度算法（Round-Robin），依次执行各任务
    let mut remain = index_mod; // 剩余未完成的任务数
    let mut i = 0usize; // 当前任务索引
    while remain > 0 {
        let tcb = &mut tcbs[i];
        if !tcb.finish {
            loop {
                // 【抢占式调度】设置时钟中断：12500 个时钟周期后触发
                // 当 coop feature 启用时，跳过此步（协作式调度，不使用时钟中断）
                #[cfg(not(feature = "coop"))]
                tg_sbi::set_timer(time::read() as u64 + 12500);

                // 切换到 U-mode 执行用户程序
                // execute() 会恢复用户寄存器并执行 sret
                // 当用户程序触发 Trap 后返回到这里
                unsafe { tcb.execute() };

                // 读取 scause 寄存器判断 Trap 原因
                use scause::*;
                let finish = match scause::read().cause() {
                    // ─── 时钟中断：时间片用完，切换到下一个任务 ───
                    Trap::Interrupt(Interrupt::SupervisorTimer) => {
                        // 清除时钟中断（设置为最大值，避免立即再次触发）
                        tg_sbi::set_timer(u64::MAX);
                        log::trace!("app{i} timeout");
                        false // 不结束任务，切换到下一个
                    }
                    // ─── 系统调用：用户程序执行了 ecall 指令 ───
                    Trap::Exception(Exception::UserEnvCall) => {
                        use task::SchedulingEvent as Event;
                        match tcb.handle_syscall() {
                            // 普通系统调用（如 write）：处理完成后继续运行当前任务
                            Event::None => continue,
                            // exit 系统调用：任务主动退出
                            Event::Exit(code) => {
                                log::info!("app{i} exit with code {code}");
                                true
                            }
                            // yield 系统调用：任务主动让出 CPU
                            Event::Yield => {
                                log::debug!("app{i} yield");
                                false // 不结束任务，切换到下一个
                            }
                            // 不支持的系统调用：杀死任务
                            Event::UnsupportedSyscall(id) => {
                                log::error!("app{i} call an unsupported syscall {}", id.0);
                                true
                            }
                        }
                    }
                    // ─── 其他异常（如非法指令、页错误等）：杀死应用 ───
                    Trap::Exception(e) => {
                        log::error!("app{i} was killed by {e:?}");
                        true
                    }
                    // ─── 外部中断：处理 VirtIO 设备的输入事件 ───
                    Trap::Interrupt(Interrupt::SupervisorExternal) => {
                        let irq = plic::claim();
                        if irq != 0 {
                            log::debug!("External interrupt: irq={}", irq);
                            if irq >= 1 && irq <= 8 {
                                unsafe {
                                    if let Some(input) = INPUT_CONTEXT.as_mut() {
                                        input.ack_interrupt();
                                        tcb.signal_pending = true;
                                    }
                                }
                            }
                            plic::complete(irq);
                        }
                        false // 不结束任务，继续执行
                    }
                    // ─── 未预期的中断：杀死应用 ───
                    Trap::Interrupt(ir) => {
                        log::error!("app{i} was killed by an unexpected interrupt {ir:?}");
                        true
                    }
                };

                // 如果任务结束（退出或被杀死），标记为已完成
                if finish {
                    tcb.finish = true;
                    remain -= 1;
                }
                break;
            }
        }
        // 轮转到下一个任务（循环取模）
        i = (i + 1) % index_mod;
    }

    // 所有用户程序执行完毕，关机
    tg_sbi::shutdown(false)
}

// ========== panic 处理 ==========

/// panic 处理函数：打印错误信息后以异常状态关机。
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("{info}");
    tg_sbi::shutdown(true)
}

// ========== 接口实现 ==========

/// 各依赖库所需接口的具体实现
mod impls {
    use tg_syscall::*;
    use crate::{GPU_CONTEXT, INPUT_CONTEXT, FB_PTR, FB_LEN, FB_WIDTH};

    /// 控制台实现：通过 SBI 逐字符输出
    pub struct Console;

    impl tg_console::Console for Console {
        #[inline]
        fn put_char(&self, c: u8) {
            tg_sbi::console_putchar(c);
        }
    }

    /// 系统调用上下文实现
    pub struct SyscallContext;

    /// IO 系统调用实现：处理 write 系统调用
    impl IO for SyscallContext {
        #[inline]
        fn write(&self, _caller: Caller, fd: usize, buf: usize, count: usize) -> isize {
            match fd {
                // 标准输出和调试输出：将缓冲区内容打印到控制台
                STDOUT | STDDEBUG => {
                    print!("{}", unsafe {
                        core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                            buf as *const u8,
                            count,
                        ))
                    });
                    count as _
                }
                // 用于 virtio-gpu 的屏幕绘制系统调用，模拟 /dev/fb0
                4 => {
                    // buf 存放具体数据，我们目前采用简单约定：
                    // flush(): write(4, 0)
                    // paint_rect(x,y,w,h,c): write(4, &RectConfig, 20)
                    if count == 0 {
                        tg_console::log::info!("GPU_FLUSH");
                        unsafe {
                            if let Some(gpu) = GPU_CONTEXT.as_mut() {
                                let _ = gpu.flush();
                            }
                        }
                        0
                    } else if count == 20 {
                        // 20 bytes = x(u32) + y(u32) + w(u32) + h(u32) + color(u32)
                        let ptr = buf as *const u32;
                        unsafe {
                            let x = core::ptr::read(ptr) as usize;
                            let y = core::ptr::read(ptr.add(1)) as usize;
                            let w = core::ptr::read(ptr.add(2)) as usize;
                            let h = core::ptr::read(ptr.add(3)) as usize;
                            let color = core::ptr::read(ptr.add(4));
                            tg_console::log::info!("GPU_PAINT: x={}, y={}, w={}, h={}, color={:#X}", x, y, w, h, color);

                            if !FB_PTR.is_null() {
                                let fb = core::slice::from_raw_parts_mut(FB_PTR as *mut u32, FB_LEN / 4);
                                for row in y..(y + h) {
                                    for col in x..(x + w) {
                                        if col < FB_WIDTH && (row * FB_WIDTH + col) < (FB_LEN / 4) {
                                            fb[row * FB_WIDTH + col] = color;
                                        }
                                    }
                                }
                            }
                        }
                        count as _
                    } else {
                        -1
                    }
                }
                _ => {
                    tg_console::log::error!("unsupported fd: {fd}");
                    -1
                }
            }
        }

        #[inline]
        fn read(&self, _caller: Caller, fd: usize, buf: usize, count: usize) -> isize {
            match fd {
                STDIN => {
                    // 不支持控制台读取
                    -1
                }
                // 用于 virtio-input 设备的读取事件，模拟 /dev/input/event0
                3 => {
                    // struct input_event 大小通常为 24 字节
                    if count >= 24 {
                        unsafe {
                            if let Some(input) = INPUT_CONTEXT.as_mut() {
                                if let Some(event) = input.pop_pending_event() {
                                    // 模拟 input_event 结构
                                    let ptr = buf as *mut u64;
                                    // event_time: u64
                                    core::ptr::write(ptr, 0); 
                                    // type (u16), code(u16), value(u32)
                                    let _ev_ptr = buf as *mut virtio_drivers::device::input::InputEvent;
                                    // event structure is compatible if padding matches
                                    core::ptr::write_unaligned(ptr.add(1) as *mut _, event);
                                    
                                    return 24; // 返回读取的字节数
                                } else {
                                    // 无事件，返回 0 或者 EAGAIN (-2 在 tg-syscall 中通常为重试)
                                    return 0;
                                }
                            }
                        }
                        0
                    } else {
                        -1
                    }
                }
                _ => -1,
            }
        }
    }

    /// Process 系统调用实现：处理 exit 系统调用
    impl Process for SyscallContext {
        #[inline]
        fn exit(&self, _caller: Caller, _status: usize) -> isize {
            0
        }
    }

    /// Scheduling 系统调用实现：处理 yield 系统调用
    ///
    /// `sched_yield` 允许任务主动让出 CPU，是协作式调度的基础。
    /// 内核在收到 yield 后会切换到下一个就绪任务。
    impl Scheduling for SyscallContext {
        #[inline]
        fn sched_yield(&self, _caller: Caller) -> isize {
            0
        }
    }

    /// Clock 系统调用实现：处理 clock_gettime 系统调用
    ///
    /// 将 RISC-V 硬件计时器的值转换为纳秒精度的时间。
    /// QEMU virt 平台的时钟频率为 12.5 MHz（10000/125 = 80 ns/tick）。
    impl Clock for SyscallContext {
        #[inline]
        fn clock_gettime(
            &self,
            _caller: Caller,
            clock_id: ClockId,
            tp: usize,
        ) -> isize {
            match clock_id {
                ClockId::CLOCK_MONOTONIC => {
                    // 将 RISC-V time 寄存器的值转换为纳秒
                    let time = riscv::register::time::read() * 10000 / 125;
                    *unsafe { &mut *(tp as *mut TimeSpec) } = TimeSpec {
                        tv_sec: time / 1_000_000_000,
                        tv_nsec: time % 1_000_000_000,
                    };
                    0
                }
                _ => -1,
            }
        }
    }

    /// Trace 系统调用实现（练习题需要完成的部分）
    ///
    /// 当前为占位实现，返回 -1 表示未实现。
    /// 学生需要在练习中实现 trace 功能，支持：
    /// - 读取用户内存（trace_request=0）
    /// - 写入用户内存（trace_request=1）
    /// - 查询系统调用计数（trace_request=2）
    impl Trace for SyscallContext {
        #[inline]
        fn trace(
            &self,
            _caller: Caller,
            _trace_request: usize,
            _id: usize,
            _data: usize,
        ) -> isize {
            tg_console::log::info!("trace: not implemented");
            -1
        }
    }
}

/// 非 RISC-V64 架构的占位模块。
///
/// 提供编译所需的符号，使得 `cargo publish --dry-run` 在主机平台上能通过编译。
#[cfg(not(target_arch = "riscv64"))]
mod stub {
    /// 主机平台占位入口
    #[unsafe(no_mangle)]
    pub extern "C" fn main() -> i32 {
        0
    }

    /// C 运行时占位
    #[unsafe(no_mangle)]
    pub extern "C" fn __libc_start_main() -> i32 {
        0
    }

    /// Rust 异常处理人格占位
    #[unsafe(no_mangle)]
    pub extern "C" fn rust_eh_personality() {}
}
