# 多道程序与分时系统
## 思维导图
```
mindmap
  root((rCore Chapter 3))
    任务管理(Task Management)
      主体: TCB (Task Control Block)
        状态: Ready / Running / Exited
        核心: TaskContext (ra, sp, s0-s11)
      管理器: TaskManager
        FIFO 调度策略
        UPSafeCell 保证单核互斥访问
    上下文切换(Dual Context)
      TrapContext (纵向)
        目的: U ↔ S 特权级转换
        位置: 内核栈顶部
        内容: 32个通用寄存器 + CSRs
      TaskContext (横向)
        目的: S ↔ S 任务控制流切换
        位置: TCB 结构体
        内容: Callee-saved 寄存器
    多道程序机制(Loader)
      内存布局: 多 App 驻留内存
      隔离方式: 物理地址偏移 (Linker Script)
      加载器: load_apps (从 .data 到指定物理地址)
    分时抢占(Preemption)
      触发源: 硬件计时器 (mtime / mtimecmp)
      关键 CSR: sstatus, sie, stvec
      时钟中断流程
        1. set_next_trigger
        2. suspend_current_and_run_next
        3. __switch
    核心汇编(The Magic)
      __alltraps: csrrw 换栈 / 压栈
      __restore: 弹出 Context / sret 返回
      __switch: 交换 sp / 实现任务“灵魂互换”
```
## ch2-ch3的系统演进

 rCore 演进：Chapter 2 -> Chapter 3 (特权级隔离至分时多任务)


### 1. 核心维度对比

| 维度 | Chapter 2 (Batch System) | Chapter 3 (Multi-tasking) | 演进意义 |
| :--- | :--- | :--- | :--- |
| **内存布局** | 内存中仅存在一个 App | **多个 App 同时驻留内存** | 消除 App 加载时的磁盘/IO 等待时延 |
| **任务切换** | `TrapContext` (U <-> S) | **`TaskContext` (S <-> S)** | 实现内核控制流之间的平滑切换 |
| **调度触发** | App 主动退出或崩溃 | **计时器中断 (Preemption)** | 剥夺 App 的“永久占据权”，实现公平调度 |
| **栈空间** | 单个内核栈 | **每个任务拥有独立内核栈** | 支持任务状态的持久化存储与切换 |



### 2. 关键机制演进

#### A. 引入 TaskContext (任务上下文)
在 Ch2 中，我们只需处理特权级切换。在 Ch3 中，由于要实现“切走 A 任务，换上 B 任务”，必须保存内核态的执行状态。



```rust
// src/task/context.rs
#[repr(C)]
pub struct TaskContext {
    ra: usize,    // 返回地址（切换后从哪开始跑）
    sp: usize,    // 内核栈指针
    s: [usize; 12], // Callee-saved registers (s0-s11)
}
```

## TL；DR
核心矛盾： 解决 Ch2 中由于 I/O 阻塞或程序长耗时导致的 CPU 浪费。
解决方案： 引入任务（Task）概念，实现 CPU 权力的主动放弃与强制剥夺。

1. 协作式多任务 (Yield)
机制： App 发现自己需要等待（如等待输入），主动调用 sys_yield。

源码体现： 内核捕获 Trap，保存当前任务状态，从任务队列中挑选下一个任务运行。

底层： 任务上下文（TaskContext）切换，不同于 TrapContext，它只保存被调用者保存寄存器（Callee-saved regs）。

2. 分时抢占式多任务 (Preemption)
机制： 不再相信 App 会自觉放弃 CPU，利用硬件时钟中断（Timer Interrupt）。

硬核逻辑： 1. 硬件设置定时器（通过 sbi_set_timer）。
2. 时间片到，触发 S-Mode 软件中断。
3. 内核强制保存当前运行 App 现场，强行切换至下一任务。

3. 任务切换的状态机
状态转换： Ready (就绪) ↔ Running (运行) ↔ Exited (退出)。

核心组件： TaskManager。它不再像 Ch2 只是简单的索引增加，而是一个维护任务状态的队列。
## 多道程序
核心定义：内存中同时存放多个独立的程序，当一个程序因为 I/O 等原因无法继续运行时，CPU 立即切换到另一个程序执行。

工程目的：极大化 CPU 利用率。

底层行为：

内存驻留：不同于 Ch2 加载一个跑一个，Ch3 预先将所有 App 加载到内存的不同位置。

被动切换：只有当当前程序“停下来”（比如等待输入或主动 yield）时，内核才接管。

硬核痛点：由于多个程序都在内存里，必须通过 link_section 配合链接脚本，为每个程序分配不同的起始地址（在 Ch4 引入页表之前，这是物理隔离的唯一手段）。

## 分时系统
核心定义：在多道程序的基础上，引入**时间片（Time Slot）**概念。内核通过硬件时钟中断，强行剥夺当前程序的执行权，循环调度每一个程序。

工程目的：最小化 响应时间（Response Time），实现“伪并行”。

底层行为：

抢占（Preemption）：内核不再等 App 主动让位，而是依靠硬件计时器（Timer Interupt）。

快速切换：通过极其精简的汇编指令（如 rCore 中的 __switch.S）保存和恢复上下文。

硬核痛点：高频率的切换会带来 Context Switch Overhead。作为追求极致优化的工程师，你会关注切换时寄存器压栈的数量以及 L1 Cache 的刷新开销。

## os/src/loader.rs
用于将`user`程序加载到内存 区别于ch2 

这是内存布局图
```
Address             Memory Segment              Description
---------------------------------------------------------------------------
0x80000000 +--------------------------+
           |     OpenSBI / RustSBI    |  Firmware (M-Mode)
0x80020000 +--------------------------+ <--- Kernel Entry
           |      .text (RX)          |  OS Kernel Code
           +--------------------------+
           |      .rodata (R)         |  Constants & App Index Table
           +--------------------------+
           |      .data (RW)          |  Initialized Data
           |  (Embedded App Binaries) |  <-- 源数据: App 0, 1, 2... 的原始镜像
           +--------------------------+
           |      .bss (RW)           |  Uninitialized Data
           |  +--------------------+  |
           |  | Task 0 Kernel Stack|  |  8KB: 存放 App 0 的 TrapContext
           |  +--------------------+  |
           |  | Task 1 Kernel Stack|  |  8KB: 存放 App 1 的 TrapContext
           |  +--------------------+  |
           |  |        ...         |  |
           +--------------------------+
0x80400000 +--------------------------+ <--- APP_BASE_ADDRESS (Slot 0)
           |                          |
           |      App 0 Run Area      |  Active Application 0
           |      + User Stack 0      |  (Loaded from .data by load_apps)
           |                          |
0x80420000 +--------------------------+ <--- APP_BASE_ADDRESS + 1 * LIMIT (Slot 1)
           |                          |
           |      App 1 Run Area      |  Active Application 1
           |      + User Stack 1      |  (Loaded from .data by load_apps)
           |                          |
0x80440000 +--------------------------+ <--- APP_BASE_ADDRESS + 2 * LIMIT (Slot 2)
           |          ...             |
---------------------------------------------------------------------------
```

```rust
/// Load nth user app at
/// [APP_BASE_ADDRESS + n * APP_SIZE_LIMIT, APP_BASE_ADDRESS + (n+1) * APP_SIZE_LIMIT).
pub fn load_apps() {
    extern "C" {
        fn _num_app(); // 从汇编或ld中得到_num_app的头地址作为函数指针
    }
    let num_app_ptr = _num_app as usize as *const usize;
    let num_app = get_num_app();
    let app_start = unsafe { core::slice::from_raw_parts(num_app_ptr.add(1), num_app + 1) }; // 到这里为止都和ch2一样
    for i in 0..num_app {
        let base_i = get_base_i(i); // 获得第i个app的头地址
        // 清空写0
        (base_i..base_i + APP_SIZE_LIMIT)
            .for_each(|addr| unsafe { (addr as *mut u8).write_volatile(0) });
        // load app from data section to memory
		// 从.bss段copy到base_i
        let src = unsafe {
            core::slice::from_raw_parts(app_start[i] as *const u8, app_start[i + 1] - app_start[i])
        };
        let dst = unsafe { core::slice::from_raw_parts_mut(base_i as *mut u8, src.len()) };
        dst.copy_from_slice(src);
    }
    unsafe {
        asm!("fence.i");
    }
}
/// 获得应用在.data段的头地址
fn get_base_i(app_id: usize) -> usize {
    APP_BASE_ADDRESS + app_id * APP_SIZE_LIMIT
}
```
## os/src/task/
这部分用于实现**任务切换**机制

注意 与Trap不同 任务切换机制是**不切换特权级**的 由**内核的调度器**进行实现

```
应用 A (User)          |        内核 (Supervisor)         |       应用 B (User)
-----------------------------|----------------------------------|-----------------------------
                             |                                  |
 [1] 运行中...                |                                  |
 [2] 触发 Trap (ecall/计时器) --|--> [3] 控制流 A 进入内核         |
                             |        (执行 trap_handler)       |
                             |              |                   |
                             |        [4] 调用 __switch (A -> B) |
                             |              |                   |
                             |   [ 暂停 A ]  |   [ 激活 B ]       |
                             |              |                   |
                             |              +-------------------|-- [5] 之前被暂停的控制流 B
                             |                                  |       从 __switch 返回
                             |                                  |              |
                             |                                  |   [6] 执行 __restore
                             |                                  |              |
                             |                                  | <---- [7] sret 返回用户态
                             |                                  |
                             |                                  | [8] 应用 B 运行中...
                             |                                  | [9] 触发 Trap
                             |              +-------------------|-- [10] 控制流 B 再次进入内核
                             |              |                   |
                             |        [11] 调用 __switch (B -> A)
                             |              |                   
                             |   [ 激活 A ]  |   [ 暂停 B ]       
                             |              |                   
 [13] 继续运行 <---------------|-- [12] 控制流 A 从 __switch 返回
 (App A 毫无察觉)              |        (执行 __restore)         
-----------------------------|----------------------------------|-----------------------------
```
### switch.rs
rust对`__switch`指令的封装

```rust
use super::TaskContext;
use core::arch::global_asm;

global_asm!(include_str!("switch.S"));
// 两个参数对应a0,a1
extern "C" {
    /// Switch to the context of `next_task_cx_ptr`, saving the current context
    /// in `current_task_cx_ptr`.
    pub fn __switch(current_task_cx_ptr: *mut TaskContext, next_task_cx_ptr: *const TaskContext);
}

```

### switch.S
汇编实现

内核先逐个保存`current_task_cx_ptr`中的寄存器信息 再恢复`next_task_cx_ptr`的寄存器
```asm
.altmacro
.macro SAVE_SN n
    sd s\n, (\n+2)*8(a0)
.endm
.macro LOAD_SN n
    ld s\n, (\n+2)*8(a1)
.endm
    .section .text
    .globl __switch
__switch: // a0与a1是它的两个参数
    # __switch(
    #     current_task_cx_ptr: *mut TaskContext,
    #     next_task_cx_ptr: *const TaskContext
    # )
    # save kernel stack of current task
    sd sp, 8(a0) // 将当前栈指针存入current_task_cx_ptr.sp
    # save ra & s0~s11 of current execution
    sd ra, 0(a0) // 将返回地址存入current_task_cx_ptr.ra
    .set n, 0
    .rept 12 // 保存s0到s11
        SAVE_SN %n
        .set n, n + 1
    .endr
    # restore ra & s0~s11 of next execution
    ld ra, 0(a1) // 恢复`next_task_cx_ptr`
    .set n, 0
    .rept 12
        LOAD_SN %n
        .set n, n + 1
    .endr
    # restore kernel stack of next task
    ld sp, 8(a1)
    ret


```

### context.rs
保存**任务寄存器的上下文信息**
```rust
#[repr(C)]
pub struct TaskContext {
    ra: usize,
    sp: usize,
    s: [usize; 12],
}

impl TaskContext {
	// 保存传入的sp 并将ra设置为__restore的入口
    pub fn goto_restore(kstack_ptr: usize) -> Self {
        extern "C" { fn __restore(); }
        Self {
            ra: __restore as usize,
            sp: kstack_ptr,
            s: [0; 12],
        }
    }
}
```
### task.rs
定义了任务的状态 与**TCB**

```rust
use super::TaskContext;
#[derive(Copy, Clone)]
pub struct TaskControlBlock { // TCB
    pub task_status: TaskStatus, // 任务状态
    pub task_cx: TaskContext, // 任务上下文
}

#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    UnInit, // 未初始化
    Ready, // 准备运行
    Running, // 正在运行
    Exited, // 已退出
}

```
### mod.rs
定义与实现了**全局的任务管理器**
```rust
pub struct TaskManager {
    /// total number of tasks
    num_app: usize, // App数量
    /// use inner value to get mutable access
    inner: UPSafeCell<TaskManagerInner>, // 内部实现
}

/// Inner of Task Manager
pub struct TaskManagerInner { //内部实现
    /// task list
    tasks: [TaskControlBlock; MAX_APP_NUM], // 任务的数组
    /// id of current `Running` task
    current_task: usize, // 目前任务的idx
}
```

全局初始化

```rust
lazy_static! {
    /// Global variable: TASK_MANAGER
    pub static ref TASK_MANAGER: TaskManager = {
        let num_app = get_num_app();
        let mut tasks = [TaskControlBlock {
            task_cx: TaskContext::zero_init(),
            task_status: TaskStatus::UnInit,
        }; MAX_APP_NUM];
        for (i, task) in tasks.iter_mut().enumerate() {
            task.task_cx = TaskContext::goto_restore(init_app_cx(i));
            task.task_status = TaskStatus::Ready;
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}

```

impl方法实现
```
stateDiagram-v2
    [*] --> UnInit
    
    UnInit --> Ready: initialize
    
    Ready --> Running: run_as_next
    
    Running --> Ready: yield
    Running --> Exited: exit
    
    Exited --> [*]
```

```rust
impl TaskManager {
    // 
    fn run_first_task(&self) -> ! {
	// 锁定调度器取出task0
        let mut inner = self.inner.exclusive_access();
        let task0 = &mut inner.tasks[0];
        task0.task_status = TaskStatus::Running;
        let next_task_cx_ptr = &task0.task_cx as *const TaskContext;
        drop(inner); //释放锁
        let mut _unused = TaskContext::zero_init(); //构造上一个任务的上下文 不过第一个任务是没有上文的
        
        unsafe {
            __switch(&mut _unused as *mut TaskContext, next_task_cx_ptr);
        }
        panic!("unreachable in run_first_task!");
    }

    /// 把当前任务的状态设置为Ready
    fn mark_current_suspended(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status = TaskStatus::Ready;
    }

    /// 设置为退出
    fn mark_current_exited(&self) {
        let mut inner = self.inner.exclusive_access();
        let current = inner.current_task;
        inner.tasks[current].task_status = TaskStatus::Exited;
    }

	/// 找下一个运行的任务
    fn find_next_task(&self) -> Option<usize> {
        let inner = self.inner.exclusive_access();
        let current = inner.current_task;
        (current + 1..current + self.num_app + 1)
            .map(|id| id % self.num_app)
            .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
    }

    /// Switch current `Running` task to the task we have found,
    /// or there is no `Ready` task and we can exit with all applications completed
    fn run_next_task(&self) {
        if let Some(next) = self.find_next_task() {
            let mut inner = self.inner.exclusive_access();
            let current = inner.current_task;
            inner.tasks[next].task_status = TaskStatus::Running; // 下一个设置为running
            inner.current_task = next;  // 当前任务设置为下一个任务
            let current_task_cx_ptr = &mut inner.tasks[current].task_cx as *mut TaskContext;
            let next_task_cx_ptr = &inner.tasks[next].task_cx as *const TaskContext;
            drop(inner);
            // before this, we should drop local variables that must be dropped manually
            unsafe {
                __switch(current_task_cx_ptr, next_task_cx_ptr); 
            }
            // go back to user mode
        } else {
            panic!("All applications completed!");
        }
    }
}
/// 标记当前为suspend 然后跑下一个
pub fn suspend_current_and_run_next() {
    mark_current_suspended();
    run_next_task();
}
/// 标记当前exited 然后跑下一个
pub fn exit_current_and_run_next() {
    mark_current_exited();
    run_next_task();
}
```
## yiled/exit系统调用
yield 是进程主动触发的“权力让渡”，它通过触发内核上下文切换（__switch），将 CPU 执行权从当前任务交还给调度器，从而允许其他就绪任务运行，本质上是协作式多任务的基础。

流程图
```
App A (User Mode)          |          Kernel (Supervisor Mode)          |        App B (User Mode)
===========================================================================================================
  [1] 执行逻辑...                 |                                            |
      a7 = SYS_YIELD             |                                            |
      ecall -------------------->| [2] __alltraps (Entry.S):                   |
                                 |     - sscratch 交换 sp (切到内核栈A)         |
                                 |     - 压栈保存 TrapContext_A                |
                                 |     - 调用 trap_handler(TrapContext_A)      |
                                 |               |                            |
                                 | [3] sys_yield (Rust):                      |
                                 |     - 修改 TaskA 状态为 Ready               |
                                 |     - 调用 __switch(&cx_A, &cx_B)           |
                                 |               |                            |
                                 | [4] __switch (Asm): <----------------------|---- [之前某时刻 Task B 暂停处]
                                 |     - 保存 Callee-saved 至 cx_A             |
                                 |     - 交换 sp: sp_A -> sp_B (!!!核心切换!!!) |
                                 |     - 从 cx_B 恢复 Callee-saved             |
                                 |     - ret (跳转至 B 的 ra 寄存器地址)        |
                                 |               |                            |
                                 | [5] 控制流 B 恢复:                          |
                                 |     - 回到之前 B 调用 __switch 的下一行      |
                                 |     - 退出 sys_yield / trap_handler         |
                                 |     - 执行 __restore (Entry.S):             |
                                 |       - 从内核栈B 弹出 TrapContext_B         |
                                 |       - sret <-----------------------------|--- [6] 恢复运行 App B
                                 |                                            |        (pc = B 的 sepc)
===========================================================================================================
```
### os/src/syscall/process.rs
实现`sys_yield` 与`sys_exit`
```rust
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next(); 
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}
```
## 分时系统
现代的任务调度算法基本都是抢占式的，它要求每个应用只能连续执行一段时间，然后内核就会将它强制性切换出去。 一般将 时间片 (Time Slice) 作为应用连续执行时长的度量单位，每个时间片可能在毫秒量级。 简单起见，我们使用 时间片轮转算法 (RR, Round-Robin) 来对应用进行调度。

RISCV中 处理器维护了时钟计数器mtime 以及一个CSR mtimecmp. 若mtime超过了CSR mtimecmp 就会触发一次时钟中断
### os/src/timer.rs

```rust
//! RISC-V timer-related functionality

use crate::config::CLOCK_FREQ;
use crate::sbi::set_timer;
use riscv::register::time;

const TICKS_PER_SEC: usize = 100;
#[allow(dead_code)]
const MSEC_PER_SEC: usize = 1000;
#[allow(dead_code)]
const MICRO_PER_SEC: usize = 1_000_000;

/// 取得mtime计数器的值
pub fn get_time() -> usize {
    time::read()
}

/// 获取毫秒单位的计数器的值
#[allow(dead_code)]
pub fn get_time_ms() -> usize {
    time::read() * MSEC_PER_SEC / CLOCK_FREQ
}

/// 获得纳秒单位的计数器的值
#[allow(dead_code)]
pub fn get_time_us() -> usize {
    time::read() * MICRO_PER_SEC / CLOCK_FREQ
}

/// 调用set_timer函数设置mtimecmp
/// 首先读取当前 mtime 的值，然后计算出 10ms 之内计数器的增量，再将 mtimecmp 设置为二者的和。 这样，10ms 之后一个 S 特权级时钟中断就会被触发。
pub fn set_next_trigger() {
    set_timer(get_time() + CLOCK_FREQ / TICKS_PER_SEC);
}

```

### os/src/sbi.rs
设置mtimecmp
```rust
/// use sbi call to set timer
pub fn set_timer(timer: usize) {
    sbi_call(SBI_SET_TIMER, timer, 0, 0);
}
```
## gettime系统调用
### os/src/syscall/process.rs
获得当前事件 保存在ts中
```rust
/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}
```
## 抢占式调度
### os/src/trap/mod.rs
新增一个分支，触发了 S 特权级时钟中断时，重新设置计时器， 调用 suspend_current_and_run_next 函数暂停当前应用并切换到下一个。


```rust
#[no_mangle]
pub fn trap_handler(cx: &mut TrapContext) -> &mut TrapContext {
    let scause = scause::read(); // get trap cause
    let stval = stval::read(); // get extra value
                               // trace!("into {:?}", scause.cause());
    match scause.cause() {
        Trap::Exception(Exception::UserEnvCall) => {
            // jump to next instruction anyway
            cx.sepc += 4;
            // get system call return value
            cx.x[10] = syscall(cx.x[17], [cx.x[10], cx.x[11], cx.x[12]]) as usize;
        }
        Trap::Exception(Exception::StoreFault) | Trap::Exception(Exception::StorePageFault) => {
            println!("[kernel] PageFault in application, bad addr = {:#x}, bad instruction = {:#x}, kernel killed it.", stval, cx.sepc);
            exit_current_and_run_next();
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            println!("[kernel] IllegalInstruction in application, kernel killed it.");
            exit_current_and_run_next();
        }
		// 新增的分支
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            set_next_trigger();
            suspend_current_and_run_next();
        }
        _ => {
            panic!(
                "Unsupported trap {:?}, stval = {:#x}!",
                scause.cause(),
                stval
            );
        }
    }
    cx
}
/// 设置sie.stie 使得S特权级的时钟中断不会被屏蔽
pub fn enable_timer_interrupt() {
    unsafe {
        sie::set_stimer();
    }
}
```
## 实验3
### 题目
获取任务信息
在 ch3 中，我们的系统已经能够支持多个任务分时轮流运行，我们希望引入一个新的系统调用 ``sys_trace``（ID 为 410）用来追踪当前任务系统调用的历史信息，并做对应的修改。定义如下。

```
fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize
```
调用规范：
这个系统调用有三种功能，根据 trace_request 的值不同，执行不同的操作：

如果 trace_request 为 0，则 id 应被视作 *const u8 ，表示读取当前任务 id 地址处一个字节的无符号整数值。此时应忽略 data 参数。返回值为 id 地址处的值。

如果 trace_request 为 1，则 id 应被视作 *mut u8 ，表示写入 data （作为 u8，即只考虑最低位的一个字节）到该用户程序 id 地址处。返回值应为0。

如果 trace_request 为 2，表示查询当前任务调用编号为 id 的系统调用的次数，返回值为这个调用次数。本次调用也计入统计 。

否则，忽略其他参数，返回值为 -1。

说明：
你可能会注意到，这个调用的读写并不安全，使用不当可能导致崩溃。这是因为在下一章节实现地址空间之前，系统中缺乏隔离机制。所以我们 不要求你实现安全检查机制，只需通过测试用例即可 。

你还可能注意到，这个系统调用读写本任务内存的功能并不是很有用。这是因为作业的灵感来源 syscall 主要依靠 trace 功能追踪其他任务的信息，但在本章节我们还没有进程、线程等概念，所以简化了操作，只要求追踪自身的信息。

#### 解答
为了添加一个系统调用 我们需要在os/src/syscall/mod.rs里处理

这个函数是所有**系统调用的入口**。 所以应该在这里添加一个逻辑。

由于trace自己也是一个**系统调用** 而加入在这里是可以正确处理trace系统调用的次数的。


```rust

pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
+    add_syscall_times(syscall_id); // 加入一个添加系统调用次数的call
    match syscall_id {
        SYSCALL_WRITE => sys_write(args[0], args[1] as *const u8, args[2]),
        SYSCALL_EXIT => sys_exit(args[0] as i32),
        SYSCALL_YIELD => sys_yield(),
        SYSCALL_GET_TIME => sys_get_time(args[0] as *mut TimeVal, args[1]),
        SYSCALL_TRACE => sys_trace(args[0], args[1], args[2]),
        _ => panic!("Unsupported syscall_id: {}", syscall_id),
    }
}

```

然后我们需要在**TCB**结构体添加一个**全局系统调用计数**

在os/src/task/task.rs中 添加**调用次数表**

```rust
#[derive(Copy, Clone)]
pub struct TaskControlBlock {
    /// The task status in it's lifecycle
    pub task_status: TaskStatus,
    /// The task context
    pub task_cx: TaskContext,
    /// 当前任务调用的系统调用次数
+    pub task_syscall_times: [usize;500], // 添加每个系统调用的调用次数表
    
}
```

在os/src/task/mod.rs中 添加相应的**初始化** 以及添加**系统调用次数**的函数

```rust
lazy_static! {
    /// Global variable: TASK_MANAGER
    pub static ref TASK_MANAGER: TaskManager = {
        let num_app = get_num_app();
        let mut tasks = [TaskControlBlock {
            task_cx: TaskContext::zero_init(),
            task_status: TaskStatus::UnInit,
+ 	    task_syscall_times: [0;500], //初始化添加
        }; MAX_APP_NUM];
        for (i, task) in tasks.iter_mut().enumerate() {
            task.task_cx = TaskContext::goto_restore(init_app_cx(i));
            task.task_status = TaskStatus::Ready;
        }
        TaskManager {
            num_app,
            inner: unsafe {
                UPSafeCell::new(TaskManagerInner {
                    tasks,
                    current_task: 0,
                })
            },
        }
    };
}

```

添加impl
```rust
impl TaskManager{
// 获取次数
	fn get_syscall_times(&self,id: usize) -> usize{
		let inner = self.inner.exclusive_access();
		let current_task = inner.current_task;
		inner.tasks[current_task].task_syscall_times[id]
    }
	// 次数+1
    fn add_syscall_times(&self,id: usize) {
		let mut inner = self.inner.exclusive_access();
		let current_task = inner.current_task;
		inner.tasks[current_task].task_syscall_times[id] += 1;
    }
}
```

添加全局函数
```rust
/// get the syscall times
pub fn get_syscall_times(id:usize) -> usize{
    TASK_MANAGER.get_syscall_times(id)
}

/// add syscall times + 1
pub fn add_syscall_times(id: usize) {
    TASK_MANAGER.add_syscall_times(id)
}

```

最后在os/src/syscall/process.rs添加实现

```rust
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    match _trace_request {
	// 获取id地址的值
	0 => {
	    let ptr = _id as *const u8;
	    unsafe {
		*ptr as isize
	    }
	},
	1 => {
	    let ptr = _id as *mut u8;
	    let val = (_data & 0xFF) as u8;
	    unsafe {
		*ptr = val;
	    }
	    0
	},
	2 => {
	    get_syscall_times(_id) as isize
	},
	_ => {
	    -1
	}
    }
}

```

### 分析trap.S的__alltraps和__restore

trap.S
```asm
.altmacro
.macro SAVE_GP n
    sd x\n, \n*8(sp)
.endm
.macro LOAD_GP n
    ld x\n, \n*8(sp)
.endm
    .section .text
    .globl __alltraps
    .globl __restore
    .align 2
__alltraps:
    csrrw sp, sscratch, sp
    # now sp->kernel stack, sscratch->user stack
    # allocate a TrapContext on kernel stack
    addi sp, sp, -34*8
    # save general-purpose registers
    sd x1, 1*8(sp)
    # skip sp(x2), we will save it later
    sd x3, 3*8(sp)
    # skip tp(x4), application does not use it
    # save x5~x31
    .set n, 5
    .rept 27
        SAVE_GP %n
        .set n, n+1
    .endr
    # we can use t0/t1/t2 freely, because they were saved on kernel stack
    csrr t0, sstatus
    csrr t1, sepc
    sd t0, 32*8(sp)
    sd t1, 33*8(sp)
    # read user stack from sscratch and save it on the kernel stack
    csrr t2, sscratch
    sd t2, 2*8(sp)
    # set input argument of trap_handler(cx: &mut TrapContext)
    mv a0, sp
    call trap_handler

__restore:
    # now sp->kernel stack(after allocated), sscratch->user stack
    # restore sstatus/sepc
    ld t0, 32*8(sp) // 从TrapContext加载sstatus
    ld t1, 33*8(sp) // 从TrapContext加载sepc
    ld t2, 2*8(sp) // 从TrapContext加载用户栈指针
    csrw sstatus, t0
    csrw sepc, t1
    csrw sscratch, t2
    # restore general-purpuse registers except sp/tp
    ld x1, 1*8(sp)
    ld x3, 3*8(sp)
    .set n, 5
    .rept 27
        LOAD_GP %n
        .set n, n+1
    .endr
    # release TrapContext on kernel stack
    addi sp, sp, 34*8
    # now sp->kernel stack, sscratch->user stack
    csrrw sp, sscratch, sp
    sret
```

1. 刚进入__restore时 sp代表了什么值 指出__restore的两个使用情景

要搞清楚这个问题 我们重新回顾 __restore 是用在哪里的 是做什么的

`将 CPU 的状态从“内核态陷阱处理现场”恢复到“用户态执行现场”，并完成特权级的平滑切换（S-mode -> U-mode）。`


在os/src/trap/context.rs的TsakContext的goto_restore里

```rust
#[repr(C)]
/// task context structure containing some registers
pub struct TaskContext {
    /// Ret position after task switching
    ra: usize,
    /// Stack pointer
    sp: usize,
    /// s0-11 register, callee saved
    s: [usize; 12],
}

/// Create a new task context with a trap return addr and a kernel stack pointer
    pub fn goto_restore(kstack_ptr: usize) -> Self {
        extern "C" {
            fn __restore();
        }
        Self {
            ra: __restore as usize,
            sp: kstack_ptr,
            s: [0; 12],
        }
		}
```

所依刚进入__restore时 sp指向当前任务内核栈上TrapContext

在任务初始启动时会使用 在常规的Trap或者系统调用返回时会启用

### 这几行汇编代码特殊处理了哪些寄存器？这些寄存器的的值对于进入用户态有何意义？请分别解释。

```asm
ld t0, 32*8(sp)
ld t1, 33*8(sp)
ld t2, 2*8(sp)
csrw sstatus, t0
csrw sepc, t1
csrw sscratch, t2
```

处理了CSR寄存器: sstatus sepc sscractch

意义在于

sstatus：定义特权级“回位”后的状态

sepc：指定用户态的入口地址

sscratch：安置内核栈指针（sp）的救命稻草
### 为何跳过了 x2 和 x4？
```asm
ld x1, 1*8(sp)
ld x3, 3*8(sp)
.set n, 5
.rept 27
   LOAD_GP %n
   .set n, n+1
.endr
```


x2 是 栈指针 (Stack Pointer)。在执行 __restore 汇编时，我们正处于一个极其微妙的状态：正在利用当前的 sp 指向的内存（内核栈）来恢复其他寄存器。

逻辑自洽性：如果你在循环中通过 ld x2, 2*8(sp) 提前恢复了 sp，那么 sp 的值会瞬间从“内核栈地址”变为“用户栈地址”。

后果：由于后续还有 20 多个寄存器（x5-x31）等待从栈中弹出，一旦 sp 变了，后续的 ld xn, n*8(sp) 指令将会去用户态的内存地址里读取数据，这会导致内核直接因为非法地址访问而崩溃，或者加载到错误的数据。

特殊处理：因此，sp 必须在所有其他通用寄存器恢复完毕后，作为最后一步（通过 ld sp, 2*8(sp)）进行切换。

x4 是 线程指针 (Thread Pointer)。在 RISC-V 的 no_std 开发和 rCore 架构中，tp 通常有特殊用途

多核/环境标识：在某些内核设计中（尤其是 Gentoo 玩家喜欢的底层优化场景），tp 寄存器常被用来存放当前 CPU 的哈特 ID (Hart ID) 或局部变量偏移。

TLS (Thread Local Storage)：即使在内核态，tp 有时也用于指向当前核的私有数据结构。

不稳定性：如果在 __restore 这种敏感的特权级转换期随意从栈上覆盖 tp，可能会破坏内核对当前硬件线程状态的感知。

惯例：在 rCore 第 3 章的简单实现中，用户态通常不使用 tp，或者 tp 的值在内核处理 Trap 过程中不需要被改写，因此为了节省一次昂贵的内存加载（ld）开销，选择了跳过。
### 该指令之后，sp 和 sscratch 中的值分别有什么意义？
```asm
csrrw sp,sscratch , sp
```

执行这行指令后，sp 和 sscratch 的值发生了原子性交换。它们的意义由 “当前处于什么阶段” 决定。

Trap 进入时（从 User 到 Kernel）这是 __alltraps 的第一条指令。sp指向内核栈（Kernel Stack）。内核现在终于拿到了属于自己的栈空间，可以开始执行压栈保存 TrapContext 的操作了。sscratch指向用户栈（User Stack）。原先应用 A 的栈指针被暂时“寄托”在这里，等待后续被存入 TrapContext.x[2]。

Trap 返回时（从 Kernel 到 User）这是 __restore 结尾，切换回用户态之前的关键步骤。sp指向用户栈（User Stack）。CPU 恢复了应用 A 运行时的栈环境，准备好执行 sret。sscratch指向内核栈（Kernel Stack）。内核栈指针被重新换回到“备用仓库” sscratch 中，为下一次发生的 Trap 埋下伏笔。
### __restore：中发生状态切换在哪一条指令？为何该指令执行之后会进入用户态？
sret

当 CPU 执行 sret 时，硬件内部会自动触发以下一系列原子操作：

特权级回转 (Privilege Level Transition)： CPU 会读取 sstatus 寄存器中的 SPP (Supervisor Previous Privilege) 字段。

在进入 __restore 之前，我们已经通过 ld t0, 32*8(sp) 和 csrw sstatus, t0 将 SPP 设置为了 User (0)。

执行 sret 时，硬件看到 SPP=0，便会将当前的特权级从 Supervisor 切换回 User。

程序计数器同步 (PC Recovery)： 硬件将 pc 指针直接设置为 sepc 寄存器中的值。

我们在 __restore 中已经预先将用户程序的入口（或被中断处）加载进了 sepc。

中断使能恢复： 硬件将 sstatus 中的 SPIE (Supervisor Previous Interrupt Enable) 拷贝回 SIE 位，恢复用户态下的中断响应状态
### 从 U 态进入 S 态是哪一条指令发生的？
ecall
## 示例问题
rCore 任务管理与上下文切换深度解析

## 1. 为什么 `switch.S` (上下文切换) 中没有 `ecall`？

在 RISC-V 架构下，`ecall` 的本质是**改变特权级（Privilege Level）**。

* **ecall 的作用**：它是一个特权级转换的“门”，负责从 **User Mode (U)** 纵向跳入 **Supervisor Mode (S)**。
* **switch.S 的语境**：上下文切换（`__switch`）发生在**内核态（S-mode）内部**。此时 CPU 已经在处理内核逻辑（如 `sys_yield` 或 `trap_handler`），不再需要通过 `ecall` 去跨越特权级边界。
* **底层逻辑**：任务切换是内核在“自言自语”，它只是把当前内核控制流的寄存器保存起来，然后强行修改 `sp` 寄存器去“白嫖”另一个任务的内核栈，这纯粹是内存数据的搬运和寄存器的赋值，不需要触发异常陷阱。

---

## 2. 在任务切换过程中 `ra` 和 `sp` 做了什么？

在 `__switch` 执行的一瞬间，`ra` 和 `sp` 决定了任务的“生死”与“复活”：

### `sp` (Stack Pointer) - 物理灵魂的载体
* **保存**：将 CPU 当前的 `sp` 写入当前任务 A 的 `TaskContext`。这意味着任务 A 运行到了哪一层内核调用栈都被“冻结”在此地址。
* **切换**：将 CPU 的 `sp` 修改为目标任务 B 的 `TaskContext` 中预存的值。**执行完这一行指令，CPU 就已经站在了任务 B 的栈空间上。**

### `ra` (Return Address) - 执行流的指向标
* **保存**：记录任务 A 调用 `__switch` 之后的下一条指令地址。
* **复活**：加载任务 B 之前保存的 `ra`。当 `__switch` 最后执行 `ret` 指令时，CPU 会跳转到这个 `ra`。
* **魔术时刻**：对于新创建的任务，其 `ra` 会被手动初始化为 `__restore` 的入口地址，从而实现从内核态直接“顺滑”降级回用户态。



---

## 3. Trap 与 Task 的关系

你可以把它们的关系理解为 **“动作”** 与 **“主体”**：

* **Task 是主体**：它拥有自己的代码、数据、用户栈和**独立的内核栈**。
* **Trap 是动作**：它是 Task 唯一能够进入内核的手段（无论是主动 `ecall` 还是被动时钟中断）。
* **生命周期绑定**：在 `rCore` 中，每个 Trap 流程都是“寄生”在某个具体的 Task 之中的。当 Trap 发生时，它利用的是**当前 Task 的内核栈**来存放临时数据。
* **切换的契机**：任务切换通常发生在 Trap 的处理过程中。没有 Trap 进入内核，调度器就无法获得 CPU 控制权，也就无法发起 Task 切换。

---

## 4. TrapContext 与 TaskContext 的区别与关系

这是最容易混淆的两个底层结构。

### 核心区别对照表

| 特性 | TrapContext (陷阱上下文) | TaskContext (任务上下文) |
| :--- | :--- | :--- |
| **存在目的** | 跨特权级状态保存 (U <-> S) | 内核态任务切换 (S <-> S) |
| **保存内容** | **全部** 32 个通用寄存器 + CSRs (sepc/sstatus) | **仅 Callee-saved** 寄存器 (ra, sp, s0-s11) |
| **存放位置** | 任务内核栈的**顶部** | 任务控制块 **TCB** 中 |
| **触发机制** | 硬件/ecall 指令 (被动/主动陷阱) | 函数调用 `__switch` (内核调度) |

### 两者的协作关系

在一个完整的任务切换链条中，它们呈现出“嵌套”关系：

1.  **进入**：Task A 触发 Trap，CPU 将 A 的现场保存到 A 的 **TrapContext**（在栈顶）。
2.  **暂停**：内核调度器调用 `__switch`，将当前内核栈的状态保存到 A 的 **TaskContext**（在 TCB）。
3.  **交接**：CPU 切换 `sp` 到 Task B 的内核栈。
4.  **恢复**：从 B 的 **TaskContext** 恢复内核现场，返回到 B 的 Trap 处理流程。
5.  **退出**：执行 `__restore`，从 B 的栈顶弹出 **TrapContext**，`sret` 回到 B 的用户态。
