# 多道程序与分时系统
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
```

## 示例问题
### 1. 为什么switch.S也就是上下文切换里没有ecall
