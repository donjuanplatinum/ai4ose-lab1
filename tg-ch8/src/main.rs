//! # 第八章：并发
//!
//! 本章在第七章"进程间通信与信号"的基础上，引入了 **线程** 和 **同步原语**。
//!
//! ## 核心概念
//!
//! ### 1. 线程（Thread）
//!
//! 将原来的"进程"拆分为两个独立的抽象：
//! - **Process（进程）**：管理共享资源（地址空间、文件描述符表、同步原语列表、信号）
//! - **Thread（线程）**：管理执行状态（上下文、TID）
//!
//! 同一进程的多个线程共享地址空间，但各自有独立的用户栈和执行上下文。
//!
//! ### 2. 同步原语
//!
//! - **Mutex（互斥锁）**：保证临界区互斥访问
//! - **Semaphore（信号量）**：P/V 操作，支持计数型资源管理
//! - **Condvar（条件变量）**：配合互斥锁使用，支持线程等待/唤醒
//!
//! ### 3. 线程阻塞
//!
//! 当线程尝试获取已被占用的锁/信号量时，内核将其标记为**阻塞态**，
//! 从就绪队列中移除。当持有者释放资源时，唤醒等待队列中的线程。
//!
//! ## 与第七章的主要区别
//!
//! | 特性 | 第七章 | 第八章 |
//! |------|--------|--------|
//! | 执行单元 | Process（进程即线程） | Thread（线程），Process 仅管理资源 |
//! | 管理器 | PManager | PThreadManager（进程 + 线程双层管理） |
//! | 同步 | 无 | Mutex / Semaphore / Condvar |
//! | task-manage feature | `proc` | `thread` |
//! | 新增依赖 | — | tg-sync |
//!
//! 教程阅读建议：
//!
//! - 先看 `rust_main`：抓住“线程创建 + 双层管理 + trap 分发”总流程；
//! - 再看主循环中 `SEMAPHORE_DOWN/MUTEX_LOCK/CONDVAR_WAIT` 分支：理解阻塞态切换；
//! - 最后看 `impls`：把线程、信号、同步三类系统调用如何交织串起来。

// 不使用标准库
#![no_std]
// 不使用默认 main 入口
#![no_main]
#![allow(static_mut_refs)]

#![cfg_attr(not(target_arch = "riscv64"), allow(dead_code, unused_imports))]

/// 文件系统模块：easy-fs 封装 + 统一 Fd 枚举
mod fs;
/// 进程与线程模块：Process（资源容器）和 Thread（执行单元）
mod process;
/// 处理器模块：PROCESSOR 全局管理器（PThreadManager）
mod processor;
/// VirtIO 块设备驱动
mod virtio_block;

#[macro_use]
extern crate tg_console;

#[macro_use]
extern crate alloc;

use crate::{
    fs::{read_all, FS},
    impls::{Sv39Manager, SyscallContext},
    process::{Process, Thread},
    processor::{ProcManager, ProcessorInner, ThreadManager},
};
use alloc::alloc::alloc;
use core::{alloc::GlobalAlloc, alloc::Layout, cell::UnsafeCell, mem::MaybeUninit, ptr::NonNull};
use customizable_buddy::{BuddyAllocator, LinkedListBuddy, UsizeBuddy};
use spin::Mutex;

/// 自定义全局堆分配器，使用本地打过补丁的 customizable-buddy
struct LockedHeap(Mutex<BuddyAllocator<28, UsizeBuddy, LinkedListBuddy>>);

unsafe impl Send for LockedHeap {}
unsafe impl Sync for LockedHeap {}

impl LockedHeap {
    const fn new() -> Self {
        Self(Mutex::new(BuddyAllocator::new()))
    }
}

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0.lock().allocate_layout::<()>(layout).map_or(
            core::ptr::null_mut(),
            |(p, _): (NonNull<()>, usize)| p.as_ptr() as *mut u8
        )
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            self.0.lock().deallocate_layout(NonNull::new_unchecked(ptr), layout)
        }
    }
}

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::new();

use impls::Console;
pub use processor::PROCESSOR;
use riscv::register::*;
#[cfg(not(target_arch = "riscv64"))]
use stub::Sv39;
use tg_console::log;
use tg_easy_fs::{FSManager, OpenFlags};
use tg_kernel_context::foreign::MultislotPortal;
#[cfg(target_arch = "riscv64")]
use tg_kernel_vm::page_table::Sv39;
use tg_kernel_vm::{
    page_table::{MmuMeta, VAddr, VmFlags, VmMeta, PPN, VPN},
    AddressSpace,
};
use tg_sbi;
use tg_signal::SignalResult;
use tg_syscall::Caller;
use tg_task_manage::ProcId;
use xmas_elf::ElfFile;

// ─── VirtIO-GPU / VirtIO-Input ───
#[allow(unused_imports)]
use virtio_drivers::{
    device::gpu::VirtIOGpu,
    device::input::VirtIOInput,
    transport::mmio::MmioTransport,
    transport::{DeviceType, Transport},
};
use crate::virtio_block::VirtioHal;

static mut GPU_CONTEXT: Option<VirtIOGpu<VirtioHal, MmioTransport>> = None;
static mut INPUT_CONTEXTS: [Option<VirtIOInput<VirtioHal, MmioTransport>>; 2] = [None, None];
static mut FB_PTR: *mut u8 = core::ptr::null_mut();
static mut FB_LEN: usize = 0;
static mut FB_WIDTH: usize = 0;
static mut KEY_STATES: [bool; 256] = [false; 256];


/// 构建 VmFlags
#[cfg(target_arch = "riscv64")]
const fn build_flags(s: &str) -> VmFlags<Sv39> {
    VmFlags::build_from_str(s)
}

/// 解析 VmFlags
#[cfg(target_arch = "riscv64")]
fn parse_flags(s: &str) -> Result<VmFlags<Sv39>, ()> {
    s.parse()
}

#[cfg(not(target_arch = "riscv64"))]
use stub::{build_flags, parse_flags};

// 内核入口，栈 = 32 页 = 128 KiB。
//
// 这里不再调用 tg_linker::boot0! 宏，避免外部已发布版本与 Rust 2024
// 在属性语义上的兼容差异影响本 crate 的发布校验。
#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.entry")]
unsafe extern "C" fn _start() -> ! {
    const STACK_SIZE: usize = 32 * 4096;
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

/// 物理内存容量 = 256 MiB
const MEMORY: usize = 256 << 20;
/// 堆分配器元数据（避开代码段）
#[unsafe(link_section = ".data")]
static mut HEAP_META: [u8; 4 * 1024 * 1024] = [1u8; 4 * 1024 * 1024];
/// 异界传送门所在虚页
const PROTAL_TRANSIT: VPN<Sv39> = VPN::MAX;

/// 内核地址空间的全局存储
struct KernelSpace {
    inner: UnsafeCell<MaybeUninit<AddressSpace<Sv39, Sv39Manager>>>,
}

unsafe impl Sync for KernelSpace {}

impl KernelSpace {
    const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    unsafe fn write(&self, space: AddressSpace<Sv39, Sv39Manager>) {
        unsafe { *self.inner.get() = MaybeUninit::new(space) };
    }

    unsafe fn assume_init_ref(&self) -> &AddressSpace<Sv39, Sv39Manager> {
        unsafe { &*(*self.inner.get()).as_ptr() }
    }
}

/// 内核地址空间全局实例
static KERNEL_SPACE: KernelSpace = KernelSpace::new();

/// VirtIO MMIO 设备地址范围
/// VirtIO MMIO 设备地址范围 (扩展到 8 个 slot 以支持 GPU + Input)
pub const MMIO: &[(usize, usize)] = &[(0x1000_1000, 0x00_8000)];

/// 内核主函数
///
/// 与第七章相比：
/// - 新增 `init_thread`（线程系统调用）和 `init_sync_mutex`（同步原语系统调用）
/// - 使用 `PThreadManager`（双层管理器）替代 `PManager`
/// - 初始化时同时创建 Process 和 Thread
/// - 主循环中新增**线程阻塞**处理（SEMAPHORE_DOWN/MUTEX_LOCK/CONDVAR_WAIT）
extern "C" fn rust_main() -> ! {
    let layout = tg_linker::KernelLayout::locate();
    // 步骤 1：BSS 清零
    unsafe { layout.zero_bss() };
    
    // 步骤 2：控制台和日志
    tg_console::init_console(&Console);
    tg_console::set_log_level(option_env!("LOG"));
    println!("[DEBUG] rust_main: BSS cleared and console initialized");
    tg_console::test_log();
    // 步骤 3：堆分配器
    ALLOCATOR.0.lock().init(5, NonNull::new(unsafe { HEAP_META.as_mut_ptr() as _ }).unwrap());
    unsafe {
        ALLOCATOR.0.lock().transfer(
            NonNull::new(0x81000000 as *mut u8).unwrap(),
            200 * 1024 * 1024,
        )
    };
    // 步骤 4：异界传送门
    let portal_size = MultislotPortal::calculate_size(1);
    let portal_layout = Layout::from_size_align(portal_size, 1 << Sv39::PAGE_BITS).unwrap();
    let portal_ptr = unsafe { alloc(portal_layout) };
    assert!(portal_layout.size() < 1 << Sv39::PAGE_BITS);
    // 步骤 5：内核地址空间
    kernel_space(layout, MEMORY, portal_ptr as _);
    // 步骤 6：异界传送门初始化
    let portal = unsafe { MultislotPortal::init_transit(PROTAL_TRANSIT.base().val(), 1) };
    // 步骤 7：系统调用初始化
    tg_syscall::init_io(&SyscallContext);
    tg_syscall::init_process(&SyscallContext);
    tg_syscall::init_scheduling(&SyscallContext);
    tg_syscall::init_clock(&SyscallContext);
    tg_syscall::init_signal(&SyscallContext);
    tg_syscall::init_thread(&SyscallContext);       // 本章新增：线程系统调用
    tg_syscall::init_sync_mutex(&SyscallContext);   // 本章新增：同步原语系统调用

    // ─── VirtIO-GPU / VirtIO-Input 初始化 ───
    println!("[KERNEL] Initializing VirtIO devices...");
    for i in 0..8 {
        let addr = 0x1000_1000 + i * 0x1000;
        let header_ptr = core::ptr::NonNull::new(addr as *mut ()).unwrap().cast();
        if let Ok(transport) = unsafe { MmioTransport::new(header_ptr) } {
            if transport.device_type() == DeviceType::Input {
                if let Ok(input) = VirtIOInput::<VirtioHal, MmioTransport>::new(transport) {
                    unsafe {
                        for j in 0..2 {
                            if INPUT_CONTEXTS[j].is_none() {
                                INPUT_CONTEXTS[j] = Some(input);
                                log::info!("VirtIO-Input device #{} initialized at {:#x}", j, addr);
                                break;
                            }
                        }
                    }
                }
            } else if transport.device_type() == DeviceType::GPU {
                log::info!("Found VirtIO-GPU at {:#x}, initializing...", addr);
                if let Ok(gpu) = VirtIOGpu::<VirtioHal, MmioTransport>::new(transport) {
                    unsafe {
                        GPU_CONTEXT = Some(gpu);
                        let gpu_ref = GPU_CONTEXT.as_mut().unwrap();
                        let (w, h) = gpu_ref.resolution().unwrap();
                        if let Ok(fb) = gpu_ref.setup_framebuffer() {
                            FB_PTR = fb.as_mut_ptr();
                            FB_LEN = fb.len();
                            FB_WIDTH = w as usize;
                            log::info!("VirtIO-GPU initialized: {}x{}, fb_ptr={:p}", w, h, FB_PTR);
                        }
                    }
                }
            }
        }
    }

    // 步骤 8：加载 initproc（返回 Process + Thread）
    println!("[DEBUG] Opening initproc...");
    let initproc_file = FS.open("initproc", OpenFlags::RDONLY)
        .expect("Failed to open initproc - is the disk image correct?");
    println!("[DEBUG] Reading initproc...");
    let initproc = read_all(initproc_file);
    println!("[DEBUG] initproc read (size={}), loading ELF...", initproc.len());
    if let Some((process, thread)) = Process::from_elf(ElfFile::new(initproc.as_slice()).unwrap()) {
        // 初始化双层管理器：ProcManager（进程）+ ThreadManager（线程）
        PROCESSOR.get_mut().set_proc_manager(ProcManager::new());
        PROCESSOR.get_mut().set_manager(ThreadManager::new());
        let (pid, tid) = (process.pid, thread.tid);
        PROCESSOR
            .get_mut()
            .add_proc(pid, process, ProcId::from_usize(usize::MAX));
        PROCESSOR.get_mut().add(tid, thread, pid);
    }

    // ─── 主调度循环 ───
    loop {
        let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;

        // ── Poll VirtIO-Input for key states ──
        unsafe {
            for i in 0..2 {
                if let Some(input) = crate::INPUT_CONTEXTS[i].as_mut() {
                    while let Some(event) = input.pop_pending_event() {
                        if event.event_type == 1 && (event.code as usize) < 256 {
                            crate::KEY_STATES[event.code as usize] = event.value == 1;
                        }
                    }
                }
            }
        }


        if let Some(task) = unsafe { (*processor).find_next() } {
            unsafe { task.context.execute(portal, ()) };

            match scause::read().cause() {
                // ─── 系统调用 ───
                scause::Trap::Exception(scause::Exception::UserEnvCall) => {
                    use tg_syscall::{SyscallId as Id, SyscallResult as Ret};
                    let ctx = &mut task.context.context;
                    ctx.move_next();
                    let id: Id = ctx.a(7).into();
                    let args = [ctx.a(0), ctx.a(1), ctx.a(2), ctx.a(3), ctx.a(4), ctx.a(5)];
                    let syscall_ret = tg_syscall::handle(Caller { entity: 0, flow: 0 }, id, args);

                    // ─── 信号处理 ───
                    let current_proc = unsafe { (*processor).get_current_proc().unwrap() };
                    match current_proc.signal.handle_signals(ctx) {
                        SignalResult::ProcessKilled(exit_code) => unsafe {
                            (*processor).make_current_exited(exit_code as _)
                        },
                        _ => match syscall_ret {
                            Ret::Done(ret) => match id {
                                Id::EXIT => unsafe { (*processor).make_current_exited(ret) },
                                // ─── 本章新增：同步原语阻塞处理 ───
                                // 当 semaphore_down / mutex_lock / condvar_wait 返回 -1 时，
                                // 表示资源不可用，将当前线程标记为阻塞态
                                Id::SEMAPHORE_DOWN | Id::MUTEX_LOCK | Id::CONDVAR_WAIT => {
                                    let ctx = &mut task.context.context;
                                    *ctx.a_mut(0) = ret as _;
                                    if ret == -1 {
                                        // 阻塞：从就绪队列移除，等待资源释放后唤醒
                                        unsafe { (*processor).make_current_blocked() };
                                    } else {
                                        // 成功获取：正常挂起（时间片轮转）
                                        unsafe { (*processor).make_current_suspend() };
                                    }
                                }
                                _ => {
                                    let ctx = &mut task.context.context;
                                    *ctx.a_mut(0) = ret as _;
                                    unsafe { (*processor).make_current_suspend() };
                                }
                            },
                            Ret::Unsupported(_) => {
                                log::info!("id = {id:?}");
                                unsafe { (*processor).make_current_exited(-2) };
                            }
                        },
                    }
                }
                e => {
                    log::error!("unsupported trap: {e:?}");
                    log::error!("stval = {:#x}", stval::read());
                    log::error!("sepc  = {:#x}", sepc::read());
                    unsafe { (*processor).make_current_exited(-3) };
                }
            }
        } else {
            println!("no task");
            break;
        }
    }

    tg_sbi::shutdown(false)
}

/// panic 处理
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("{info}");
    tg_sbi::shutdown(true)
}

/// 建立内核地址空间（与前几章相同）
fn kernel_space(layout: tg_linker::KernelLayout, memory: usize, portal: usize) {
    let mut space = AddressSpace::new();
    for region in layout.iter() {
        log::info!("{region}");
        use tg_linker::KernelRegionTitle::*;
        let flags = match region.title {
            Text => "X_RV",
            Rodata => "__RV",
            Data | Boot => "_WRV",
        };
        let s = VAddr::<Sv39>::new(region.range.start);
        let e = VAddr::<Sv39>::new(region.range.end);
        space.map_extern(
            s.floor()..e.ceil(),
            PPN::new(s.floor().val()),
            build_flags(flags),
        )
    }
    let s = VAddr::<Sv39>::new(layout.end());
    let e = VAddr::<Sv39>::new(layout.start() + memory);
    log::info!("(heap) ---> {:#10x}..{:#10x}", s.val(), e.val());
    space.map_extern(
        s.floor()..e.ceil(),
        PPN::new(s.floor().val()),
        build_flags("_WRV"),
    );
    space.map_extern(
        PROTAL_TRANSIT..PROTAL_TRANSIT + 1,
        PPN::new(portal >> Sv39::PAGE_BITS),
        build_flags("__G_XWRV"),
    );
    println!();
    // 映射 VirtIO MMIO 和 PLIC 区域 (0x0c00_0000 .. 0x1000_9000)
    space.map_extern(
        VPN::<Sv39>::new(0x0c000)..VPN::<Sv39>::new(0x10009),
        PPN::<Sv39>::new(0x0c000),
        build_flags("_WRV"),
    );
    unsafe { satp::set(satp::Mode::Sv39, 0, space.root_ppn().val()) };
    unsafe { KERNEL_SPACE.write(space) };
}

/// 将异界传送门映射到用户地址空间
fn map_portal(space: &AddressSpace<Sv39, Sv39Manager>) {
    let portal_idx = PROTAL_TRANSIT.index_in(Sv39::MAX_LEVEL);
    space.root()[portal_idx] = unsafe { KERNEL_SPACE.assume_init_ref() }.root()[portal_idx];
}

/// 各种接口库的实现
///
/// 与第七章相比，本章新增了：
/// - `Thread` trait（thread_create/gettid/waittid）
/// - `SyncMutex` trait（mutex/semaphore/condvar 系统调用）
/// - 所有操作通过 `ProcessorInner`（PThreadManager）进行双层管理
mod impls {
    use crate::{
        build_flags,
        fs::{read_all, Fd, FS},
        processor::ProcessorInner,
        Sv39, Thread, PROCESSOR,
    };
    use alloc::sync::Arc;
    use alloc::{alloc::alloc_zeroed, string::String, vec::Vec};
    use core::{alloc::Layout, ptr::NonNull};
    use spin::Mutex;
    use tg_console::log;
    use tg_easy_fs::{make_pipe, FSManager, OpenFlags, UserBuffer};
    use tg_kernel_vm::{
        page_table::{MmuMeta, Pte, VAddr, VmFlags, VmMeta, PPN, VPN},
        PageManager,
    };
    use tg_signal::SignalNo;
    use tg_sync::{Condvar, Mutex as MutexTrait, MutexBlocking, Semaphore};
    use tg_syscall::*;
    use tg_task_manage::{ProcId, ThreadId};
    use xmas_elf::ElfFile;

    // ─── Sv39 页表管理器 ───

    /// Sv39 页表管理器
    #[repr(transparent)]
    pub struct Sv39Manager(NonNull<Pte<Sv39>>);

    impl Sv39Manager {
        const OWNED: VmFlags<Sv39> = unsafe { VmFlags::from_raw(1 << 8) };
        #[inline]
        fn page_alloc<T>(count: usize) -> *mut T {
            unsafe {
                alloc_zeroed(Layout::from_size_align_unchecked(
                    count << Sv39::PAGE_BITS,
                    1 << Sv39::PAGE_BITS,
                ))
            }
            .cast()
        }
    }

    impl PageManager<Sv39> for Sv39Manager {
        #[inline]
        fn new_root() -> Self { Self(NonNull::new(Self::page_alloc(1)).unwrap()) }
        #[inline]
        fn root_ppn(&self) -> PPN<Sv39> { PPN::new(self.0.as_ptr() as usize >> Sv39::PAGE_BITS) }
        #[inline]
        fn root_ptr(&self) -> NonNull<Pte<Sv39>> { self.0 }
        #[inline]
        fn p_to_v<T>(&self, ppn: PPN<Sv39>) -> NonNull<T> {
            unsafe { NonNull::new_unchecked(VPN::<Sv39>::new(ppn.val()).base().as_mut_ptr()) }
        }
        #[inline]
        fn v_to_p<T>(&self, ptr: NonNull<T>) -> PPN<Sv39> {
            PPN::new(VAddr::<Sv39>::new(ptr.as_ptr() as _).floor().val())
        }
        #[inline]
        fn check_owned(&self, pte: Pte<Sv39>) -> bool { pte.flags().contains(Self::OWNED) }
        #[inline]
        fn allocate(&mut self, len: usize, flags: &mut VmFlags<Sv39>) -> NonNull<u8> {
            *flags |= Self::OWNED;
            let ptr: *mut u8 = Self::page_alloc(len);
            if ptr.is_null() {
                panic!("[DEBUG] allocate failed! requested len (pages): {}", len);
            }
            NonNull::new(ptr).unwrap()
        }
        fn deallocate(&mut self, pte: Pte<Sv39>, len: usize) -> usize {
            if self.check_owned(pte) {
                let ppn = pte.ppn();
                let ptr = self.p_to_v::<u8>(ppn).as_ptr();
                unsafe {
                    alloc::alloc::dealloc(
                        ptr,
                        Layout::from_size_align_unchecked(len << Sv39::PAGE_BITS, 1 << Sv39::PAGE_BITS),
                    );
                }
            }
            0
        }
        fn drop_root(&mut self) {
            unsafe {
                alloc::alloc::dealloc(
                    self.0.as_ptr() as _,
                    Layout::from_size_align_unchecked(1 << Sv39::PAGE_BITS, 1 << Sv39::PAGE_BITS),
                );
            }
        }
    }

    // ─── 控制台 ───

    /// 控制台实现
    pub struct Console;
    impl tg_console::Console for Console {
        #[inline]
        fn put_char(&self, c: u8) { tg_sbi::console_putchar(c); }
    }

    // ─── 系统调用实现 ───

    /// 系统调用上下文
    pub struct SyscallContext;
    const READABLE: VmFlags<Sv39> = build_flags("RV");
    const WRITEABLE: VmFlags<Sv39> = build_flags("W_V");

    /// IO 系统调用（与第七章基本相同）
    ///
    /// 注意：本章通过 `get_current_proc()` 获取当前线程所属的进程，
    /// 而非直接 `current()`，因为 fd_table 属于进程而非线程。
    impl IO for SyscallContext {
        fn write(&self, _caller: Caller, fd: usize, buf: usize, count: usize) -> isize {
            let current = PROCESSOR.get_mut().get_current_proc().unwrap();
            
            if fd == STDOUT || fd == STDDEBUG {
                if let Some(ptr) = current.address_space.translate::<u8>(VAddr::new(buf), READABLE) {
                    print!("{}", unsafe {
                        core::str::from_utf8_unchecked(core::slice::from_raw_parts(
                            ptr.as_ptr(), count,
                        ))
                    });
                    return count as _;
                } else {
                    log::error!("sys_write: buffer at {:#x} not readable", buf);
                    return -1;
                }
            } else if let Some(file) = &current.fd_table[fd] {
                let file_guard = file.lock();
                if file_guard.writable() {
                    match &*file_guard {
                        Fd::VirtioGpu => {
                            // ── Framebuffer write Page-By-Page ──
                            unsafe {
                                if !crate::FB_PTR.is_null() {
                                    let mut count_left = count.min(crate::FB_LEN);
                                    let mut buf_addr = buf;
                                    let mut fb_offset = 0;

                                    while count_left > 0 {
                                        if let Some(ptr) = current.address_space.translate::<u8>(VAddr::new(buf_addr), READABLE) {
                                            let page_offset = buf_addr % 4096;
                                            let copy_size = count_left.min(4096 - page_offset);
                                            core::ptr::copy_nonoverlapping(
                                                ptr.as_ptr(),
                                                crate::FB_PTR.add(fb_offset),
                                                copy_size,
                                            );
                                            count_left -= copy_size;
                                            buf_addr += copy_size;
                                            fb_offset += copy_size;
                                        } else {
                                            log::error!("GPU blit error: pointer unreadable at {:#x}", buf_addr);
                                            break;
                                        }
                                    }
                                    
                                    if let Some(gpu) = crate::GPU_CONTEXT.as_mut() {
                                        if let Err(e) = gpu.flush() {
                                            log::error!("GPU flush failed: {:?}", e);
                                        }
                                    }
                                }
                            }
                            return count as _;
                        }
                        _ => {
                            let mut v: Vec<&'static mut [u8]> = Vec::new();
                            let mut count_left = count;
                            let mut buf_addr = buf;
                            while count_left > 0 {
                                if let Some(ptr) = current.address_space.translate::<u8>(VAddr::new(buf_addr), READABLE) {
                                    let page_offset = buf_addr % 4096;
                                    let copy_size = count_left.min(4096 - page_offset);
                                    unsafe {
                                        v.push(core::slice::from_raw_parts_mut(ptr.as_ptr(), copy_size));
                                    }
                                    count_left -= copy_size;
                                    buf_addr += copy_size;
                                } else {
                                    log::error!("write: translation failed at {:#x}", buf_addr);
                                    break;
                                }
                            }
                            if v.is_empty() { return -1; }
                            return file_guard.write(UserBuffer::new(v)) as _;
                        }
                    }
                } else {
                    log::error!("sys_write: file at fd {} not writable", fd);
                    return -1;
                }
            } else {
                log::error!("unsupported fd: {fd}");
                return -1;
            }
        }

        fn read(&self, _caller: Caller, fd: usize, buf: usize, count: usize) -> isize {
            let current = PROCESSOR.get_mut().get_current_proc().unwrap();
            
            if fd == STDIN {
                if let Some(ptr) = current.address_space.translate::<u8>(VAddr::new(buf), WRITEABLE) {
                    let mut ptr = ptr.as_ptr();
                    for _ in 0..count {
                        unsafe { *ptr = tg_sbi::console_getchar() as u8; ptr = ptr.add(1); }
                    }
                    return count as _;
                } else {
                    log::error!("sys_read: buffer at {:#x} not writeable", buf);
                    return -1;
                }
            } else if let Some(file) = &current.fd_table[fd] {
                let file_guard = file.lock();
                if file_guard.readable() {
                    match &*file_guard {
                        Fd::VirtioInput => {
                            // ── VirtIO-Input KEY_STATES read ──
                            if let Some(ptr) = current.address_space.translate::<u8>(VAddr::new(buf), WRITEABLE) {
                                unsafe {
                                    let dst = core::slice::from_raw_parts_mut(ptr.as_ptr(), count.min(256));
                                    for i in 0..count.min(256) {
                                        dst[i] = if crate::KEY_STATES[i] { 1 } else { 0 };
                                    }
                                }
                                return count as _;
                            } else {
                                log::error!("sys_read: Input buffer at {:#x} not writeable", buf);
                                return -1;
                            }
                        }
                        _ => {
                            let mut v: Vec<&'static mut [u8]> = Vec::new();
                            let mut count_left = count;
                            let mut buf_addr = buf;
                            while count_left > 0 {
                                if let Some(ptr) = current.address_space.translate::<u8>(VAddr::new(buf_addr), WRITEABLE) {
                                    let page_offset = buf_addr % 4096;
                                    let copy_size = count_left.min(4096 - page_offset);
                                    unsafe {
                                        v.push(core::slice::from_raw_parts_mut(ptr.as_ptr(), copy_size));
                                    }
                                    count_left -= copy_size;
                                    buf_addr += copy_size;
                                } else {
                                    log::error!("read: translation failed at {:#x}", buf_addr);
                                    break;
                                }
                            }
                            if v.is_empty() { return -1; }
                            return file_guard.read(UserBuffer::new(v)) as _;
                        }
                    }
                } else {
                    log::error!("sys_read: file at fd {} not readable", fd);
                    return -1;
                }
            } else {
                log::error!("unsupported fd: {fd}");
                return -1;
            }
        }

        fn open(&self, _caller: Caller, path: usize, flags: usize) -> isize {
            let current = PROCESSOR.get_mut().get_current_proc().unwrap();
            if let Some(_ptr) = current.address_space.translate::<u8>(VAddr::new(path), READABLE) {
                let mut string = String::new();
                let mut vaddr = path;
                loop {
                    if let Some(ptr) = current.address_space.translate(VAddr::new(vaddr), READABLE) {
                        unsafe {
                            let ch: u8 = *ptr.as_ptr();
                            if ch == 0 { break; }
                            string.push(ch as char);
                        }
                        vaddr += 1;
                    } else {
                        log::error!("sys_open: path string unreadable at {:#x}", vaddr);
                        return -1;
                    }
                }
                if string == "/dev/gpu" {
                    let new_fd = current.fd_table.len();
                    current.fd_table.push(Some(Mutex::new(Fd::VirtioGpu)));
                    log::info!("Opened /dev/gpu as fd {}", new_fd);
                    return new_fd as isize;
                }
                if string == "/dev/input" {
                    let new_fd = current.fd_table.len();
                    current.fd_table.push(Some(Mutex::new(Fd::VirtioInput)));
                    log::info!("Opened /dev/input as fd {}", new_fd);
                    return new_fd as isize;
                }
                if let Some(file_handle) =
                    FS.open(string.as_str(), OpenFlags::from_bits(flags as u32).unwrap())
                {
                    let new_fd = current.fd_table.len();
                    current.fd_table.push(Some(Mutex::new(Fd::File((*file_handle).clone()))));
                    new_fd as isize
                } else { -1 }
            } else { log::error!("sys_open: path at {:#x} not readable", path); -1 }
        }

        #[inline]
        fn close(&self, _caller: Caller, fd: usize) -> isize {
            let current = PROCESSOR.get_mut().get_current_proc().unwrap();
            if fd >= current.fd_table.len() || current.fd_table[fd].is_none() { return -1; }
            current.fd_table[fd].take();
            0
        }

        /// pipe 系统调用
        fn pipe(&self, _caller: Caller, pipe: usize) -> isize {
            let current = PROCESSOR.get_mut().get_current_proc().unwrap();
            let (read_end, write_end) = make_pipe();
            let read_fd = current.fd_table.len();
            let write_fd = read_fd + 1;
            if let Some(mut ptr) = current.address_space
                .translate::<usize>(VAddr::new(pipe), WRITEABLE)
            { unsafe { *ptr.as_mut() = read_fd }; } else { return -1; }
            if let Some(mut ptr) = current.address_space
                .translate::<usize>(VAddr::new(pipe + core::mem::size_of::<usize>()), WRITEABLE)
            { unsafe { *ptr.as_mut() = write_fd }; } else { return -1; }
            current.fd_table.push(Some(Mutex::new(Fd::PipeRead(read_end))));
            current.fd_table.push(Some(Mutex::new(Fd::PipeWrite(write_end))));
            0
        }
    }

    /// 进程管理系统调用
    impl Process for SyscallContext {
        #[inline]
        fn exit(&self, _caller: Caller, exit_code: usize) -> isize { exit_code as isize }

        /// fork：创建子进程（返回 Process + Thread）
        fn fork(&self, _caller: Caller) -> isize {
            let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
            let current_proc = unsafe { (*processor).get_current_proc().unwrap() };
            let parent_pid = current_proc.pid;
            let (proc, mut thread) = current_proc.fork().unwrap();
            let pid = proc.pid;
            *thread.context.context.a_mut(0) = 0 as _;
            unsafe {
                (*processor).add_proc(pid, proc, parent_pid);
                (*processor).add(thread.tid, thread, pid);
            }
            pid.get_usize() as isize
        }

        /// exec：从文件系统加载新程序
        fn exec(&self, _caller: Caller, path: usize, count: usize) -> isize {
            const READABLE: VmFlags<Sv39> = build_flags("RV");
            let current = PROCESSOR.get_mut().get_current_proc().unwrap();
            current.address_space
                .translate(VAddr::new(path), READABLE)
                .map(|ptr| unsafe {
                    core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr.as_ptr(), count))
                })
                .and_then(|name| FS.open(name, OpenFlags::RDONLY))
                .map_or_else(
                    || {
                        log::error!("unknown app, select one in the list: ");
                        FS.readdir("").unwrap().into_iter().for_each(|app| println!("{app}"));
                        println!();
                        -1
                    },
                    |fd| { current.exec(ElfFile::new(&read_all(fd)).unwrap()); 0 },
                )
        }

        fn wait(&self, _caller: Caller, pid: isize, exit_code_ptr: usize) -> isize {
            let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
            let current = unsafe { (*processor).get_current_proc().unwrap() };
            const WRITABLE: VmFlags<Sv39> = build_flags("W_V");
            if let Some((dead_pid, exit_code)) =
                unsafe { (*processor).wait(ProcId::from_usize(pid as usize)) }
            {
                if let Some(mut ptr) = current.address_space
                    .translate::<i32>(VAddr::new(exit_code_ptr), WRITABLE)
                { unsafe { *ptr.as_mut() = exit_code as i32 }; }
                return dead_pid.get_usize() as isize;
            } else { return -1; }
        }

        fn getpid(&self, _caller: Caller) -> isize {
            PROCESSOR.get_mut().get_current_proc().unwrap().pid.get_usize() as _
        }
    }

    impl Scheduling for SyscallContext {
        #[inline]
        fn sched_yield(&self, _caller: Caller) -> isize { 0 }
    }

    impl Clock for SyscallContext {
        #[inline]
        fn clock_gettime(&self, _caller: Caller, clock_id: ClockId, tp: usize) -> isize {
            const WRITABLE: VmFlags<Sv39> = build_flags("W_V");
            match clock_id {
                ClockId::CLOCK_MONOTONIC => {
                    if let Some(mut ptr) = PROCESSOR.get_mut().get_current_proc().unwrap()
                        .address_space.translate(VAddr::new(tp), WRITABLE)
                    {
                        let time = riscv::register::time::read() * 10000 / 125;
                        *unsafe { ptr.as_mut() } = TimeSpec {
                            tv_sec: time / 1_000_000_000,
                            tv_nsec: time % 1_000_000_000,
                        };
                        0
                    } else { log::error!("ptr not readable"); -1 }
                }
                _ => -1,
            }
        }
    }

    /// 信号系统调用（与第七章相同）
    impl Signal for SyscallContext {
        fn kill(&self, _caller: Caller, pid: isize, signum: u8) -> isize {
            if let Some(target_task) = PROCESSOR.get_mut()
                .get_proc(ProcId::from_usize(pid as usize))
            {
                if let Ok(signal_no) = SignalNo::try_from(signum) {
                    if signal_no != SignalNo::ERR {
                        target_task.signal.add_signal(signal_no);
                        return 0;
                    }
                }
            }
            -1
        }

        fn sigaction(&self, _caller: Caller, signum: u8, action: usize, old_action: usize) -> isize {
            if signum as usize > tg_signal::MAX_SIG { return -1; }
            let current = PROCESSOR.get_mut().get_current_proc().unwrap();
            if let Ok(signal_no) = SignalNo::try_from(signum) {
                if signal_no == SignalNo::ERR { return -1; }
                if old_action as usize != 0 {
                    if let Some(mut ptr) = current.address_space.translate(VAddr::new(old_action), WRITEABLE) {
                        if let Some(signal_action) = current.signal.get_action_ref(signal_no) {
                            *unsafe { ptr.as_mut() } = signal_action;
                        } else { return -1; }
                    } else { return -1; }
                }
                if action as usize != 0 {
                    if let Some(ptr) = current.address_space.translate(VAddr::new(action), READABLE) {
                        if !current.signal.set_action(signal_no, &unsafe { *ptr.as_ptr() }) { return -1; }
                    } else { return -1; }
                }
                return 0;
            }
            -1
        }

        fn sigprocmask(&self, _caller: Caller, mask: usize) -> isize {
            PROCESSOR.get_mut().get_current_proc().unwrap().signal.update_mask(mask) as isize
        }

        fn sigreturn(&self, _caller: Caller) -> isize {
            let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
            let current = unsafe { (*processor).get_current_proc().unwrap() };
            let current_thread = unsafe { (*processor).current().unwrap() };
            if current.signal.sig_return(&mut current_thread.context.context) { 0 } else { -1 }
        }
    }

    /// 线程系统调用（**本章新增**）
    impl tg_syscall::Thread for SyscallContext {
        /// thread_create：在当前进程中创建新线程
        ///
        /// 为新线程分配独立的用户栈（从高地址向下搜索未映射的页面），
        /// 创建新的执行上下文，入口为 entry，参数为 arg。
        fn thread_create(&self, _caller: Caller, entry: usize, arg: usize) -> isize {
            let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
            let current_proc = unsafe { (*processor).get_current_proc().unwrap() };
            // 从最高用户栈位置向下搜索空闲的页表区域
            let mut vpn = VPN::<Sv39>::new((1 << 26) - 2);
            let addrspace = &mut current_proc.address_space;
            loop {
                let idx = vpn.index_in(Sv39::MAX_LEVEL);
                if !addrspace.root()[idx].is_valid() { break; }
                vpn = VPN::<Sv39>::new(vpn.val() - 3);
            }
            // 分配 2 页用户栈
            let stack = unsafe {
                alloc_zeroed(Layout::from_size_align_unchecked(
                    2 << Sv39::PAGE_BITS, 1 << Sv39::PAGE_BITS,
                ))
            };
            addrspace.map_extern(vpn..vpn + 2, PPN::new(stack as usize >> Sv39::PAGE_BITS), build_flags("U_WRV"));
            let satp = (8 << 60) | addrspace.root_ppn().val();
            let mut context = tg_kernel_context::LocalContext::user(entry);
            *context.sp_mut() = (vpn + 2).base().val();
            *context.a_mut(0) = arg;
            let thread = Thread::new(satp, context);
            let tid = thread.tid;
            unsafe { (*processor).add(tid, thread, current_proc.pid); }
            tid.get_usize() as _
        }

        /// gettid：获取当前线程 TID
        fn gettid(&self, _caller: Caller) -> isize {
            PROCESSOR.get_mut().current().unwrap().tid.get_usize() as _
        }

        /// waittid：等待指定线程退出
        fn waittid(&self, _caller: Caller, tid: usize) -> isize {
            let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
            let current_thread = unsafe { (*processor).current().unwrap() };
            if tid == current_thread.tid.get_usize() { return -1; }
            if let Some(exit_code) = unsafe { (*processor).waittid(ThreadId::from_usize(tid)) } {
                exit_code
            } else { -1 }
        }
    }

    /// 同步原语系统调用（**本章新增**）
    ///
    /// 实现 Mutex、Semaphore、Condvar 的创建和操作。
    /// 这些同步原语存储在 Process 的列表中，由所有线程共享。
    impl SyncMutex for SyscallContext {
        /// 创建信号量（初始计数 = res_count）
        fn semaphore_create(&self, _caller: Caller, res_count: usize) -> isize {
            let current_proc = PROCESSOR.get_mut().get_current_proc().unwrap();
            let id = if let Some(id) = current_proc.semaphore_list.iter().enumerate()
                .find(|(_, item)| item.is_none()).map(|(id, _)| id)
            {
                current_proc.semaphore_list[id] = Some(Arc::new(Semaphore::new(res_count)));
                id
            } else {
                current_proc.semaphore_list.push(Some(Arc::new(Semaphore::new(res_count))));
                current_proc.semaphore_list.len() - 1
            };
            id as isize
        }

        /// V 操作：释放信号量，唤醒等待线程
        fn semaphore_up(&self, _caller: Caller, sem_id: usize) -> isize {
            let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
            let current_proc = unsafe { (*processor).get_current_proc().unwrap() };
            let sem = Arc::clone(current_proc.semaphore_list[sem_id].as_ref().unwrap());
            if let Some(tid) = sem.up() {
                unsafe { (*processor).re_enque(tid); }
            }
            0
        }

        /// P 操作：获取信号量，不可用则阻塞
        fn semaphore_down(&self, _caller: Caller, sem_id: usize) -> isize {
            let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
            let current = unsafe { (*processor).current().unwrap() };
            let tid = current.tid;
            let current_proc = unsafe { (*processor).get_current_proc().unwrap() };
            let sem = Arc::clone(current_proc.semaphore_list[sem_id].as_ref().unwrap());
            if !sem.down(tid) { -1 } else { 0 }
        }

        /// 创建互斥锁（blocking=true 为阻塞锁）
        fn mutex_create(&self, _caller: Caller, blocking: bool) -> isize {
            let new_mutex: Option<Arc<dyn MutexTrait>> = if blocking {
                Some(Arc::new(MutexBlocking::new()))
            } else { None };
            let current_proc = PROCESSOR.get_mut().get_current_proc().unwrap();
            if let Some(id) = current_proc.mutex_list.iter().enumerate()
                .find(|(_, item)| item.is_none()).map(|(id, _)| id)
            {
                current_proc.mutex_list[id] = new_mutex;
                id as isize
            } else {
                current_proc.mutex_list.push(new_mutex);
                current_proc.mutex_list.len() as isize - 1
            }
        }

        /// 解锁，唤醒等待线程
        fn mutex_unlock(&self, _caller: Caller, mutex_id: usize) -> isize {
            let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
            let current_proc = unsafe { (*processor).get_current_proc().unwrap() };
            let mutex = Arc::clone(current_proc.mutex_list[mutex_id].as_ref().unwrap());
            if let Some(tid) = mutex.unlock() {
                unsafe { (*processor).re_enque(tid); }
            }
            0
        }

        /// 加锁，已被占用则阻塞
        fn mutex_lock(&self, _caller: Caller, mutex_id: usize) -> isize {
            let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
            let current = unsafe { (*processor).current().unwrap() };
            let tid = current.tid;
            let current_proc = unsafe { (*processor).get_current_proc().unwrap() };
            let mutex = Arc::clone(current_proc.mutex_list[mutex_id].as_ref().unwrap());
            if !mutex.lock(tid) { -1 } else { 0 }
        }

        /// 创建条件变量
        fn condvar_create(&self, _caller: Caller, _arg: usize) -> isize {
            let current_proc = PROCESSOR.get_mut().get_current_proc().unwrap();
            let id = if let Some(id) = current_proc.condvar_list.iter().enumerate()
                .find(|(_, item)| item.is_none()).map(|(id, _)| id)
            {
                current_proc.condvar_list[id] = Some(Arc::new(Condvar::new()));
                id
            } else {
                current_proc.condvar_list.push(Some(Arc::new(Condvar::new())));
                current_proc.condvar_list.len() - 1
            };
            id as isize
        }

        /// 唤醒一个等待线程
        fn condvar_signal(&self, _caller: Caller, condvar_id: usize) -> isize {
            let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
            let current_proc = unsafe { (*processor).get_current_proc().unwrap() };
            let condvar = Arc::clone(current_proc.condvar_list[condvar_id].as_ref().unwrap());
            if let Some(tid) = condvar.signal() {
                unsafe { (*processor).re_enque(tid); }
            }
            0
        }

        /// 等待条件变量（释放锁 + 阻塞 + 重新获取锁）
        fn condvar_wait(&self, _caller: Caller, condvar_id: usize, mutex_id: usize) -> isize {
            let processor: *mut ProcessorInner = PROCESSOR.get_mut() as *mut ProcessorInner;
            let current = unsafe { (*processor).current().unwrap() };
            let tid = current.tid;
            let current_proc = unsafe { (*processor).get_current_proc().unwrap() };
            let condvar = Arc::clone(current_proc.condvar_list[condvar_id].as_ref().unwrap());
            let mutex = Arc::clone(current_proc.mutex_list[mutex_id].as_ref().unwrap());
            let (flag, waking_tid) = condvar.wait_with_mutex(tid, mutex);
            if let Some(waking_tid) = waking_tid {
                unsafe { (*processor).re_enque(waking_tid); }
            }
            if !flag { -1 } else { 0 }
        }

        /// 死锁检测（TODO 练习题）
        fn enable_deadlock_detect(&self, _caller: Caller, is_enable: i32) -> isize {
            tg_console::log::info!("enable_deadlock_detect: is_enable = {is_enable}, not implemented");
            -1
        }
    }
}

/// 非 RISC-V64 架构的占位实现
#[cfg(not(target_arch = "riscv64"))]
mod stub {
    use tg_kernel_vm::page_table::{MmuMeta, VmFlags};

    /// Sv39 占位类型
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
    pub struct Sv39;
    impl MmuMeta for Sv39 {
        const P_ADDR_BITS: usize = 56;
        const PAGE_BITS: usize = 12;
        const LEVEL_BITS: &'static [usize] = &[9, 9, 9];
        const PPN_POS: usize = 10;
        #[inline]
        fn is_leaf(value: usize) -> bool { value & 0b1110 != 0 }
    }
    /// 构建 VmFlags 占位
    pub const fn build_flags(_s: &str) -> VmFlags<Sv39> { unsafe { VmFlags::from_raw(0) } }
    /// 解析 VmFlags 占位
    pub fn parse_flags(_s: &str) -> Result<VmFlags<Sv39>, ()> { Ok(unsafe { VmFlags::from_raw(0) }) }

    #[unsafe(no_mangle)]
    pub extern "C" fn main() -> i32 { 0 }
    #[unsafe(no_mangle)]
    pub extern "C" fn __libc_start_main() -> i32 { 0 }
    #[unsafe(no_mangle)]
    pub extern "C" fn rust_eh_personality() {}
}
