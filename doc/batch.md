# 批处理系统

在早期计算机时代，程序员需要亲自上阵挂磁带、拨开关。这种模式下，CPU 处于严重的空闲状态（等待人类缓慢的操作）。批处理系统的出现，本质上是引入了一个**“监控程序”（Monitor）**——这也是现代操作系统内核的雏形。


## AI助手TL;DR：本章目标
本章名为**“批处理系统”。核心任务是在第一章“脱离 OS 的裸机程序”基础上，构建一个能自动、连续执行多个用户态程序**的初级操作系统。

实现特权级隔离：利用 RISC-V 的 U-Mode（用户态）和 S-Mode（内核态），确保用户程序不能随意执行内核指令（如关机或修改页表）。

构建 Trap 机制：实现 CPU 上下文的保存与恢复，处理用户态到内核态的强制跳转（系统调用/异常）。

App 管理器：在内核二进制中“硬编码”加载多个用户 App，并实现一个简单的调度逻辑，当一个程序结束时，自动加载运行下一个。

### 目标
本章的核心是从“孤立的裸机程序”进化为**“具备特权级保护的批处理系统”**。你不仅要让代码跑起来，更要建立起一套“内核管控 App”的秩序。

🎯 必须达成的硬核目标：
实现特权级切换（Privilege Barrier）：

利用 RISC-V 的 sstatus 寄存器强制区分 U-Mode（用户态）和 S-Mode（内核态）。

达成标准：用户 App 尝试执行 sret 或关机等特权指令时，必须能触发非法指令异常，而不是直接关机。

构建 Trap 上下文切换机制（Context Switch）：

在 trap.S 中手动编写汇编代码，完成通用寄存器的压栈与出栈。

达成标准：当 App 执行 ecall 后，内核能获取其寄存器状态，处理完系统调用后，App 能精确返回到下一条指令并恢复所有寄存器。

App 内存镜像布局与自动化加载：

编写 build.rs 将多个用户程序二进制文件打包进内核。

达成标准：内核能够根据符号（如 _num_app）找到 App 数据，并将其 memmove 到指定的运行地址（如 0x80400000）。

实现最小化系统调用子集：

封装 SYS_WRITE（通过内核转发给 SBI）和 SYS_EXIT。

达成标准：用户 App 能够通过 ecall 输出字符，并在结束后告知内核切换下一个程序。

## AI助手本章思维导图
```
mindmap
  root((rCore Ch2: 批处理系统))
    特权级机制 (Privilege)
      U-Mode (User): 受限环境, 运行 App
      S-Mode (Supervisor): 内核环境, 掌控硬件
      特权级切换: ecall (U->S), sret (S->U)
    App 加载与链接
      用户态库: 实现 _start, syscall 封装, println! 宏
      build.rs: 编译脚本, 将 App 二进制打包进内核 .data 段
      内存布局: 规定 App 运行的物理起始地址
    Trap 处理 (核心)
      TrapContext: 保存通用寄存器 + sstatus + sepc
      __alltraps: 汇编入口, 切换 sp 到内核栈, 保存上下文
      __restore: 汇编出口, 恢复上下文, 切换 sp 回用户栈
      trap_handler: Rust 分发中心, 处理 Syscall/Exception
    批处理逻辑
      AppManager: 维护 App 数量、ID、位置信息
      run_next_app: 加载程序至内存 -> 构建 TrapContext -> sret 启动
    系统调用 (Syscall)
      SYSCALL_WRITE (64): 打印字符串
      SYSCALL_EXIT (93): 程序正常退出, 触发加载下一个 App
```
## 源代码分析
### user/build.py
这一章中多了**用户态**的应用程序user.

在user中 有一个build.py 从build/app下读取所有的应用并编译

```python
import os

base_address = 0x80400000
step = 0x20000
linker = "src/linker.ld"

app_id = 0
apps = os.listdir("build/app")
apps.sort()
chapter = os.getenv("CHAPTER")
mode = os.getenv("MODE", default = "release")
if mode == "release" :
	mode_arg = "--release"
else :
    mode_arg = ""

for app in apps:
    app = app[: app.find(".")]
    os.system(
        "cargo rustc --bin %s %s -- -Clink-args=-Ttext=%x"
        % (app, mode_arg, base_address + step * app_id)
    )
    print(
        "[build.py] application %s start with address %s"
        % (app, hex(base_address + step * app_id))
    )
    if chapter == '3':
        app_id = app_id + 1

```

在user目录下有很多个user程序 因为目前阶段的操作系统我们**并没有**实现高级的**MMU**和**分页机制** 所以需要像第一章那样去**静态**的分配每个程序的位置。

这里的step=0x20000是指每个程序的头距离0x20000
```
[  物理内存地址空间  ]
        |
        v
+-----------------------+ <--- 0x80200000 (Kernel Start)
|                       |
|      内核 (OS) 代码      |  (运行在 S-Mode)
|                       |
+-----------------------+ <--- 0x80400000 (base_address)
|                       |
|   App 0 (HelloWorld)  |  <--- 链接地址: 0x80400000
|   (Max 128KB)         |
|                       |
+-----------------------+ <--- 0x80420000 (base + 1*step)
|                       |
|   App 1 (UserShell)   |  <--- 链接地址: 0x80420000
|   (Max 128KB)         |
|                       |
+-----------------------+ <--- 0x80440000 (base + 2*step)
|                       |
|   App 2 (MatrixMul)   |  <--- 链接地址: 0x80440000
|   (Max 128KB)         |
|                       |
+-----------------------+ <--- 0x80460000 (base + 3*step)
|          ...          |
+-----------------------+
|  (未使用的物理内存)     |
+-----------------------+
```


### os/build.rs与os/src/link_app.S
```rust
//! Building applications linker

use std::fs::{read_dir, File};
use std::io::{Result, Write};

fn main() {
    println!("cargo:rerun-if-changed=../user/src/");
    println!("cargo:rerun-if-changed={}", TARGET_PATH);
    insert_app_data().unwrap();
}

static TARGET_PATH: &str = "../user/build/bin/";

/// get app data and build linker
fn insert_app_data() -> Result<()> {
    let mut f = File::create("src/link_app.S").unwrap();
    let mut apps: Vec<_> = read_dir("../user/build/bin/")
        .unwrap()
        .into_iter()
        .map(|dir_entry| {
            let mut name_with_ext = dir_entry.unwrap().file_name().into_string().unwrap();
            name_with_ext.drain(name_with_ext.find('.').unwrap()..name_with_ext.len());
            name_with_ext
        })
        .collect();
    apps.sort();

    writeln!(
        f,
        r#"
    .align 3
    .section .data
    .global _num_app
_num_app:
    .quad {}"#,
        apps.len()
    )?;

    for i in 0..apps.len() {
        writeln!(f, r#"    .quad app_{}_start"#, i)?;
    }
    writeln!(f, r#"    .quad app_{}_end"#, apps.len() - 1)?;

    for (idx, app) in apps.iter().enumerate() {
        println!("app_{}: {}", idx, app);
        writeln!(
            f,
            r#"
    .section .data
    .global app_{0}_start
    .global app_{0}_end
app_{0}_start:
    .incbin "{2}{1}.bin"
app_{0}_end:"#,
            idx, app, TARGET_PATH
        )?;
    }
    Ok(())
}

```

这个build.rs的作用是**编译用户程序** 

首先它根据脚本 创建了一个link_app.S的**汇编** 将用户程序 **嵌入到内核**

这是内存的布局
```
[  内核数据段 .data  ]
        |
        v
+-----------------------+ <--- 符号 _num_app
|       App 数量 (n)     |  (.quad n)
+-----------------------+
|    app_0_start 地址    |  (地址表项 0)
+-----------------------+
|    app_1_start 地址    |  (地址表项 1)
+-----------------------+
|          ...          |
+-----------------------+
|    app_n-1_end 地址   |  (最后一个 App 的结尾地址)
+-----------------------+ <--- 符号 app_0_start
|                       |
|   App 0 二进制数据     |  (由 .incbin 注入)
|                       |
+-----------------------+ <--- 符号 app_0_end / app_1_start
|                       |
|   App 1 二进制数据     |
|                       |
+-----------------------+
```

我们来观察生成的link_app.S汇编 它将内存布局设置好后 **rust代码会访问里面的地址**.
```asm

    .align 3
    .section .data
    .global _num_app
_num_app:
    .quad 7
    .quad app_0_start
    .quad app_1_start
    .quad app_2_start
    .quad app_3_start
    .quad app_4_start
    .quad app_5_start
    .quad app_6_start
    .quad app_6_end

    .section .data
    .global app_0_start
    .global app_0_end
app_0_start:
    .incbin "../user/build/bin/ch2b_bad_address.bin"
app_0_end:

    .section .data
    .global app_1_start
    .global app_1_end
app_1_start:
    .incbin "../user/build/bin/ch2b_bad_instructions.bin"
app_1_end:

    .section .data
    .global app_2_start
    .global app_2_end
app_2_start:
    .incbin "../user/build/bin/ch2b_bad_register.bin"
app_2_end:

    .section .data
    .global app_3_start
    .global app_3_end
app_3_start:
    .incbin "../user/build/bin/ch2b_hello_world.bin"
app_3_end:

    .section .data
    .global app_4_start
    .global app_4_end
app_4_start:
    .incbin "../user/build/bin/ch2b_power_3.bin"
app_4_end:

    .section .data
    .global app_5_start
    .global app_5_end
app_5_start:
    .incbin "../user/build/bin/ch2b_power_5.bin"
app_5_end:

    .section .data
    .global app_6_start
    .global app_6_end
app_6_start:
    .incbin "../user/build/bin/ch2b_power_7.bin"
app_6_end:

```

### 用户程序
用户程序都在user的src

#### 库
首先来看用户程序们的库函数

- console.rs

这个文件里实现了print和println

- lang_items.rs

这个文件里实现了panic_handler

- lib.rs

定义入口点 (_start)：接管程序启动，手动清空 .bss，初始化堆分配器，并解析 argc/argv 参数。

提供堆内存管理：利用 buddy_system_allocator 在用户态实现了一个 16KB 的静态堆空间，支持 Vec、Box 等 alloc 容器。

封装 Syscall ABI：将内核提供的 ecall 接口包装成 Rust 风格的强类型函数（如 fork, exec, mmap, mutex 等）。

支持多线程与同步：提供了用户态的互斥锁（Mutex）、信号量（Semaphore）和条件变量（Condvar）的接口。

信号机制（Signal）：实现了类似 POSIX 的信号处理框架（sigaction, kill）。


- syscall.rs

定义了系统调用 系统调用可以看syscall.md


### os/src/batch.rs
这个文件是实现了**应用管理器** `AppManager`

加入了应用管理器后 内存布局为
```
Address          Memory Segment             Description
---------------------------------------------------------------------------
0x80000000 +--------------------------+
           |      OpenSBI / RustSBI   |  Firmware (M-Mode)
0x80020000 +--------------------------+ <--- Kernel Entry
           |      .text (RX)          |  Kernel Code (Trap Handler, etc.)
           +--------------------------+
           |      .rodata (R)         |  Constants, App Index Table
           +--------------------------+
           |      .data (RW)          |  Initialized Data
           |  (Embedded App Binaries) |  <-- 7个.incbin 就在这里
           +--------------------------+
           |      .bss (RW)           |  Uninitialized Data
           |   +------------------+   |
           |   |   KERNEL_STACK   |   |  8KB (4096-aligned)
           |   +------------------+   |
           |   |    USER_STACK    |   |  8KB (4096-aligned)
           |   +------------------+   |
           +--------------------------+
0x80400000 +--------------------------+ <--- APP_BASE_ADDRESS
           |                          |
           |     Current Running      |  Active Application Area
           |          App             |  (Loaded by copy_from_slice)
           |                          |
0x80420000 +--------------------------+ <--- APP_SIZE_LIMIT Boundary
           |                          |
           |      Available RAM       |  Free Memory
           |                          |
           +--------------------------+
```
#### 这是appManager的实现
```rust
struct AppManager {
    num_app: usize, // app数量
    current_app: usize, // 目前运行的app的编号
    app_start: [usize; MAX_APP_NUM + 1], // 每个app在内存的头
}
```


每次使用`load_app`会情况`APP_BASE_ADDRESS`到`APP_SIZE_LIMIT`的内容 也就是从`0x80400000`到`0x80420000` 然后把 `.data`里面的程序复制过来

注意 最后调用了一个`fence.i`汇编指令 它的作用是清空`I-Cache`缓存

当你执行 copy_from_slice（搬运 App 代码）时，CPU 把 App 的二进制流看作是“数据”，写入的是 D-Cache。 然而，当你跳转到 0x80400000 开始执行时，CPU 会通过 I-Cache 去取指令。

问题在于：D-Cache 和 I-Cache 之间通常是不直接同步的。如果不处理，CPU 拿到的可能是 I-Cache 里的旧指令（可能是上一个 App 的残余，或者是全 0），导致程序崩溃或执行错误。

执行 fence.i 后，硬件会保证：

写回 D-Cache：确保之前所有对存储器（内存）的写入操作对“取指操作”可见。

无效化 I-Cache：清空（Invalidate）旧的指令缓存，迫使 CPU 下次取指时必须从内存（或 L2 Cache）中重新读取最新的数据。

流水线清空：由于指令可能已经被预取进流水线，fence.i 通常会触发流水线刷新（Pipeline Flush），确保后续执行的是新指令。

#### impl
```rust
impl AppManager {
    pub fn print_app_info(&self) {
        println!("[kernel] num_app = {}", self.num_app);
        for i in 0..self.num_app {
            println!(
                "[kernel] app_{} [{:#x}, {:#x})",
                i,
                self.app_start[i],
                self.app_start[i + 1]
            );
        }
    }

    unsafe fn load_app(&self, app_id: usize) {
        if app_id >= self.num_app {
            println!("All applications completed!");
            use crate::board::QEMUExit;
            crate::board::QEMU_EXIT_HANDLE.exit_success();
        }
        println!("[kernel] Loading app_{}", app_id);
        // clear app area
        core::slice::from_raw_parts_mut(APP_BASE_ADDRESS as *mut u8, APP_SIZE_LIMIT).fill(0);
        let app_src = core::slice::from_raw_parts(
            self.app_start[app_id] as *const u8,
            self.app_start[app_id + 1] - self.app_start[app_id],
        );
        let app_dst = core::slice::from_raw_parts_mut(APP_BASE_ADDRESS as *mut u8, app_src.len());
        app_dst.copy_from_slice(app_src);
        // Memory fence about fetching the instruction memory
        // It is guaranteed that a subsequent instruction fetch must
        // observes all previous writes to the instruction memory.
        // Therefore, fence.i must be executed after we have loaded
        // the code of the next app into the instruction memory.
        // See also: riscv non-priv spec chapter 3, 'Zifencei' extension.
        asm!("fence.i");
    }

    pub fn get_current_app(&self) -> usize {
        self.current_app
    }

    pub fn move_to_next_app(&mut self) {
        self.current_app += 1;
    }
}
```

这里使用lazy_static宏初始化了AppManager

通过_num_app函数指针指向link_app.S的符号 
#### AppManager的初始化
```rust
lazy_static! {
    static ref APP_MANAGER: UPSafeCell<AppManager> = unsafe {
        UPSafeCell::new({
            extern "C" {
                fn _num_app();
            }
            let num_app_ptr = _num_app as usize as *const usize; //目前num_app_ptr指向_num_app
            let num_app = num_app_ptr.read_volatile(); // 拿到app数量
            let mut app_start: [usize; MAX_APP_NUM + 1] = [0; MAX_APP_NUM + 1]; // 初始化app_start 得到
            let app_start_raw: &[usize] =
                core::slice::from_raw_parts(num_app_ptr.add(1), num_app + 1); // num_app_ptr.add(1)刚好跳过了num_app的位置 然后长度为num_app+1
            app_start[..=num_app].copy_from_slice(app_start_raw); // 这块内存根据link的脚本 存放着每个程序的头地址 所以转换为rust的类型
            AppManager {
                num_app,
                current_app: 0,
                app_start,
            }
        })
    };
}

```

#### run_next_app
运行下一个APP 这里涉及到了异常处理
```rust
/// run next app
pub fn run_next_app() -> ! {
    let mut app_manager = APP_MANAGER.exclusive_access();
    let current_app = app_manager.get_current_app();
    unsafe {
        app_manager.load_app(current_app); // 先将现在要运行的加载进内存
    }
    app_manager.move_to_next_app(); // 将AppManager里指向下一个APP
    drop(app_manager);
    // before this we have to drop local variables related to resources manually
    // and release the resources
    extern "C" {
        fn __restore(cx_addr: usize); //  trap.S的函数
    }
    unsafe {
        __restore(KERNEL_STACK.push_context(TrapContext::app_init_context(
            APP_BASE_ADDRESS,
            USER_STACK.get_sp(),
        )) as *const _ as usize);
    }
    panic!("Unreachable in batch::run_current_app!");
}

```

#### 用户栈与内核栈
这部分是Trap触发的时候 CPU需要保存的信息

```rust

#[repr(align(4096))]
struct KernelStack {
  data: [u8; KERNEL_STACK_SIZE],
}

#[repr(align(4096))]
struct UserStack {
  data: [u8; USER_STACK_SIZE],
}

static KERNEL_STACK: KernelStack = KernelStack {
  data: [0; KERNEL_STACK_SIZE],
};
static USER_STACK: UserStack = UserStack {
  data: [0; USER_STACK_SIZE],
};
```
