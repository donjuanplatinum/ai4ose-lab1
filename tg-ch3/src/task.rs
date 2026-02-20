//! 任务管理模块
//!
//! 定义了任务控制块（Task Control Block, TCB）和调度事件，
//! 是多道程序系统的核心数据结构。
//!
//! ## 与第二章的区别
//!
//! 第二章的批处理系统中，用户上下文直接在 `rust_main` 的局部变量中管理。
//! 本章将其封装到 `TaskControlBlock` 中，每个任务拥有独立的 TCB，
//! 包含用户上下文、完成状态和独立的用户栈，支持多任务并发。
//!
//! 教程阅读建议：
//!
//! - 先看 `TaskControlBlock` 字段：理解“上下文 + 栈 + 状态位”最小任务模型；
//! - 再看 `handle_syscall`：理解系统调用结果如何映射成调度事件；
//! - 最后对照 `ch3/src/main.rs`：把“事件生成”和“事件消费”串成闭环。

use tg_kernel_context::LocalContext;
use tg_syscall::{Caller, SyscallId};

/// 任务控制块（Task Control Block, TCB）
///
/// 每个用户程序对应一个 TCB，包含：
/// - `ctx`：用户态上下文（所有通用寄存器 + 控制寄存器），用于任务切换时保存/恢复状态
/// - `finish`：任务是否已完成（退出或被杀死）
/// - `stack`：用户栈空间（8 KiB），每个任务有独立的栈
pub struct TaskControlBlock {
    /// 用户态上下文：保存 Trap 时的所有寄存器状态
    ctx: LocalContext,
    /// 任务完成标志：true 表示已退出或被杀死
    pub finish: bool,
    /// 用户栈：16 KiB（2048 个 usize = 2048 × 8 = 16384 字节）
    /// 每个任务拥有独立的栈空间，避免栈溢出影响其他任务
    stack: [usize; 2048],
    /// 注册的信号处理函数入口
    pub signal_handler: Option<usize>,
    /// 是否有未处理的信号
    pub signal_pending: bool,
    /// 备份的上下文
    pub saved_ctx: Option<LocalContext>,
}

/// 调度事件
///
/// `handle_syscall` 处理完系统调用后返回此枚举，
/// 告知主循环应该如何调度当前任务。
pub enum SchedulingEvent {
    /// 系统调用处理完成，继续执行当前任务（如 write、clock_gettime）
    None,
    /// 任务主动让出 CPU（yield 系统调用），切换到下一个任务
    Yield,
    /// 任务请求退出（exit 系统调用），附带退出码
    Exit(usize),
    /// 不支持的系统调用，附带系统调用 ID
    UnsupportedSyscall(SyscallId),
}

impl TaskControlBlock {
    /// 零值常量：用于数组初始化
    pub const ZERO: Self = Self {
        ctx: LocalContext::empty(),
        finish: false,
        stack: [0; 2048],
        signal_handler: None,
        signal_pending: false,
        saved_ctx: None,
    };

    /// 初始化一个任务
    ///
    /// - 清零用户栈
    /// - 创建用户态上下文，设置入口地址和 `sstatus.SPP = User`
    /// - 将栈指针设置为用户栈的栈顶（高地址端）
    pub fn init(&mut self, entry: usize) {
        self.stack.fill(0);
        self.finish = false;
        self.ctx = LocalContext::user(entry);
        self.signal_handler = None;
        self.signal_pending = false;
        self.saved_ctx = None;
        // 栈从高地址向低地址增长，所以 sp 指向栈顶（数组末尾之后的地址）
        *self.ctx.sp_mut() = self.stack.as_ptr() as usize + core::mem::size_of_val(&self.stack);
    }

    /// 执行此任务
    ///
    /// 恢复用户寄存器并执行 `sret` 切换到 U-mode。
    /// 当用户程序触发 Trap 后返回到此函数的调用处。
    #[inline]
    pub unsafe fn execute(&mut self) {
        if self.signal_pending {
            if let Some(handler) = self.signal_handler {
                // 如果当前没有正在处理信号，保存下当前的执行上下文并跳转到信号处理函数
                if self.saved_ctx.is_none() {
                    self.saved_ctx = Some(self.ctx.clone());
                    *self.ctx.pc_mut() = handler;
                }
            }
            self.signal_pending = false;
        }
        unsafe { self.ctx.execute() };
    }

    /// 从信号处理中返回
    pub fn sigreturn(&mut self) {
        if let Some(ctx) = self.saved_ctx.take() {
            self.ctx = ctx;
        }
    }

    /// 处理系统调用，返回调度事件
    ///
    /// 从用户上下文中提取系统调用 ID（a7 寄存器）和参数（a0-a5 寄存器），
    /// 分发到对应的处理函数，并将返回值写回 a0 寄存器。
    pub fn handle_syscall(&mut self) -> SchedulingEvent {
        use tg_syscall::{SyscallId as Id, SyscallResult as Ret};
        use SchedulingEvent as Event;

        // a7 寄存器存放 syscall ID
        let id = self.ctx.a(7).into();
        // a0-a5 寄存器存放系统调用参数
        let args = [
            self.ctx.a(0),
            self.ctx.a(1),
            self.ctx.a(2),
            self.ctx.a(3),
            self.ctx.a(4),
            self.ctx.a(5),
        ];
        match tg_syscall::handle(Caller { entity: 0, flow: 0 }, id, args) {
            Ret::Done(ret) => match id {
                // exit 系统调用：返回退出事件
                Id::EXIT => Event::Exit(self.ctx.a(0)),
                // yield 系统调用：返回让出事件
                Id::SCHED_YIELD => {
                    *self.ctx.a_mut(0) = ret as _;
                    self.ctx.move_next(); // sepc += 4，跳过 ecall 指令
                    Event::Yield
                }
                Id::RT_SIGACTION => {
                    self.signal_handler = Some(self.ctx.a(1));
                    *self.ctx.a_mut(0) = 0;
                    self.ctx.move_next();
                    Event::None
                }
                Id::RT_SIGRETURN => {
                    self.sigreturn();
                    // sigreturn 恢复了备份的 ctx，其 pc 已经是触发信号时的指令或下一条指令，不需要 move_next
                    Event::None
                }
                // 其他系统调用（如 write、clock_gettime）：继续执行
                _ => {
                    *self.ctx.a_mut(0) = ret as _;
                    self.ctx.move_next(); // sepc += 4，跳过 ecall 指令
                    Event::None
                }
            },
            // 不支持的系统调用
            Ret::Unsupported(_) => Event::UnsupportedSyscall(id),
        }
    }
}
