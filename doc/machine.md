# 裸机执行环境
在本节中 我们将把`hello,wolrd`从**用户态** 搬到**内核态**

## AI助手本章思维导图

```
graph TD
    %% Root Goal
    Goal[实现内核态 Hello World / 关机] --> Boot[1. 硬件启动与跳转流程]
    Goal --> Memory[2. 内存空间布局与对齐]
    Goal --> Runtime[3. Rust 运行时最小化支撑]

    %% Section 1: Boot Process
    subgraph Boot [启动链条]
        QEMU[QEMU Virt 模拟器] -->|固化代码跳转| RustSBI[RustSBI / M-Mode]
        RustSBI -->|Privilege Transition| S_Mode[S-Mode Kernel Entry]
        S_Mode -->|ecall| SBI_Services[SBI 服务: 关机/输出]
    end

    %% Section 2: Memory & Linking
    subgraph Memory [内存布局控制]
        LD_Script[Linker Script linker.ld] -->|BASE_ADDRESS| Entry_Align[0x80200000 物理对齐]
        LD_Script -->|Sections| Segments[.text, .data, .rodata, .bss]
        Segments -->|Specific Header| Text_Entry[.text.entry 强制置顶]
    end

    %% Section 3: Runtime
    subgraph Runtime [运行时初始化]
        Entry_ASM[entry.asm 汇编引导] -->|Initial SP| Stack_Init[栈空间初始化 64KB]
        Stack_Init -->|Jump| Rust_Main[rust_main 入口函数]
        Rust_Main -->|Memory Safety| BSS_Clear[手动清空 .bss 段]
        BSS_Clear -->|FFI| Extern_Symbols[extern C 访问链接脚本符号]
    end

    %% Key Dependencies
    Text_Entry -.->|Ensures| S_Mode
    Entry_ASM -.->|Links to| Text_Entry
    LD_Script -.->|Provides| Extern_Symbols
```
## AI助手困难点与知识链条分析
## 裸机启动
使用QEMU的system模拟器来模拟RISCV64计算机
```shell
qemu-system-riscv64 \
            -machine virt \
            -nographic \
            -bios $(BOOTLOADER) \
            -device loader,file=$(KERNEL_BIN),addr=$(KERNEL_ENTRY_PA)
```

其中:
- `-bios $(BOOTLOADER)` 代表硬件加载的BootLoader 也就是RustSBI

- `-device loader,file=$(KERNEL_BIN),addr=$(KERNEL_ENTRY_PA)`代表在内存中的`KERNEL_ENTRY_PA`位置放置内核的二进制文件`KERNEL_BIN` KERNEL_BIN相当于linux的vmlinuz

执行qemu后相当于给RISCV计算机加电了

加电后的流程是这样的
### 启动流程

第一阶段： QEMU固化设备

- CPU的寄存器会清零 然后QEMU会在`0x1000`放置指令

- 生成DTB动态设备树 然后将地址存放在a1寄存器

- 跳转到`0x80000000`的RustSBI处

第二阶段: BIOS(RUSTSBI)运行

- 在virt机器中 DRAM的起始物理地址为`0x80000000`

- SBI运行在M级别

- SBI配置PMP 委托中断 设置CPU频率 定时器等硬件

第三阶段： Loader的注入与内核的托管

- QEMU跳过文件系统的加载 强行将KERNEL_BIN二进制内容复制到$(KERNEL_ENTRY_PA)处
- SBI跳转 SBI完成工作后 通过mret指令降低特权级 跳转到`0x8020000`的内核入口


## 关机功能实现
在RISC-V中 关机的功能需要与M级别的SBI交互。内核态S是无权关机的。

`ecall`指令是与S请求M的通道

其中
- `a7`寄存器指定了需要调用的模块
- `a6`寄存器指定了函数的名称
- `a0-a5`传递参数

### Rust的ecall封装
`sbi_call`函数封装了ecall的调用

注意 a对应下面的x1 所以吧which插入到x17就是把which插入到a7

即which(a7)存放需要调用的模块 arg0(a0)是第一个参数 arg1(a1)是第二个参数 arg2(a2)是第三个参数
```rust
#[inline(always)]
fn sbi_call(which: usize, arg0: usize, arg1: usize, arg2: usize) -> usize {
    let mut ret;
    unsafe {
        asm!(
            "li x16, 0",
            "ecall",
            inlateout("x10") arg0 => ret,
            in("x11") arg1,
            in("x12") arg2,
            in("x17") which,
        );
    }
    ret
}

/// use sbi call to putchar in console (qemu uart handler)
pub fn console_putchar(c: usize) {
    sbi_call(SBI_CONSOLE_PUTCHAR, c, 0, 0);
}

use crate::board::QEMUExit;
/// use sbi call to shutdown the kernel
pub fn shutdown() -> ! {
    crate::board::QEMU_EXIT_HANDLE.exit_failure();
}

```

### 关机功能实现
```rust
const SBI_SHUTDOWN: usize = 8;

pub fn shutdown() -> ! {
    sbi_call(SBI_SHUTDOWN, 0, 0, 0);
    panic!("It should shutdown!");
}

// os/src/main.rs
#[no_mangle]
extern "C" fn _start() {
    shutdown();
}
```

通过对SBI的ecall调用 实现了shutdown

这个时候我们来尝试运行 会遇到问题
```shell
# 编译生成ELF格式的执行文件
$ cargo build --release
 Compiling os v0.1.0 (/media/chyyuu/ca8c7ba6-51b7-41fc-8430-e29e31e5328f/thecode/rust/os_kernel_lab/os)
  Finished release [optimized] target(s) in 0.15s
# 把ELF执行文件转成bianary文件
$ rust-objcopy --binary-architecture=riscv64 target/riscv64gc-unknown-none-elf/release/os --strip-all -O binary target/riscv64gc-unknown-none-elf/release/os.bin

# 加载运行
$ qemu-system-riscv64 -machine virt -nographic -bios ../bootloader/rustsbi-qemu.bin -device loader,file=target/riscv64gc-unknown-none-elf/release/os.bin,addr=0x80200000
# 无法退出，风扇狂转，感觉碰到死循环
```


这是因为**入口地址**并不是0x80200000 默认的链接器脚本不会把程序入口固定在0x80200000 所以我们需要通过**链接脚本** 来修改程序的**栈空间**

首先修改`.cargo/config`来修改链接脚本
```toml
[build]
target = "riscv64gc-unknown-none-elf"

[target.riscv64gc-unknown-none-elf]
rustflags = [
    "-Clink-arg=-Tsrc/linker.ld", "-Cforce-frame-pointers=yes"
]
```

rustflags代表了
- `-Clink-arg`: rustc的codegen选项 表示 将接下来的参数**原封不动**的传递给**链接器**
- `-Tsrc/linker.ld`: -T代表指定链接脚本的路径  src/linker.ld是链接脚本地址
- `-Cforce-frame-pointers=yes`: 强制编译器为每一个函数保留**帧指针**

#### 链接脚本
```ld
OUTPUT_ARCH(riscv)
ENTRY(_start)
BASE_ADDRESS = 0x80200000;

SECTIONS
{
    . = BASE_ADDRESS;
    skernel = .;

    stext = .;
    .text : {
        *(.text.entry)
        *(.text .text.*)
    }

    . = ALIGN(4K);
    etext = .;
    srodata = .;
    .rodata : {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }

    . = ALIGN(4K);
    erodata = .;
    sdata = .;
    .data : {
        *(.data .data.*)
        *(.sdata .sdata.*)
    }

    . = ALIGN(4K);
    edata = .;
    .bss : {
        *(.bss.stack)
        sbss = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
    }

    . = ALIGN(4K);
    ebss = .;
    ekernel = .;

    /DISCARD/ : {
        *(.eh_frame)
    }
}
```

我们来仔细的拆解这个ld（使用的LLVM ld语法 因为Rust的后端默认LLD）

##### 第一部分 全局配置
- OUTPUT_ARCH(riscv): 指定了链接器链接的平台
- ENTRY(_start): 函数的入口点符号为_start
- BASE_ADDRESS: 定义一个常量。 这个地址0x8020000是内核在S态下被引导的物理地址 所以如果BASE_ADDRESS是这个地址 那么这个程序是内核。

##### 第二部分 SECTIONS布局
这个部分是定义了各个段数据在文件和内存中的排列方式

一个基本的ELF应该有这些段

| 段          | 属性                | 说明                      |
|-------------|---------------------|---------------------------|
| .text       | AX(Alloc/Exec)      | 机器指令 也就是核心代码   |
| .data       | WA(Write/Alloc)     | 已经初始化的全局/静态变量 |
| .bss        | WA                  | 未初始化的全局变量        |
| .rodata     | A(Alloc)            | 只读的常量                |

- `.`: 代表当前程序的位置计数器 所以`. = BASE_ADDRESS` 代表所有布局都从`0x80200000`开始
- `skernel=./stext=.`: 代表定义符号skernel/stext 可以在rust里访问这个符号

- `.text : {*(.text.entry) *(.text .text.*)}`:

`.text:{}`代表创建一个名为.text的输出段

`*(.text.entry)` 代表强制将所有输入文件中的`.text.entry`放在最前面 

实际上这里的*是通配符 正常的格式的<file>(section) 

`*(.text .text.*)`: 收集.text段的所有指令

`.=ALIGN(4k)`: 将当前地址对齐到4kb

后面的指令都差不多 只不过是从设置.text段到设置.rodata .data .bss段
#### 汇编实现
这里需要rust内联汇编来**初始化栈空间**

```asm
     .section .text.entry
     .globl _start
_start:
     la sp, boot_stack_top
     call rust_main
 
     .section .bss.stack
     .globl boot_stack
boot_stack:
    .space 4096 * 16
    .globl boot_stack_top
boot_stack_top:
```

- `.section .text.entry` 这里代表了定义一个`.text.entry`代码段 配合链接脚本里的`*(.text.entry)` 确保了这段汇编指令会被放置在内存的`0x8020000`

- `.global _start` 将_start符号声明为**全局可见** 这里能让链接器找到_start. 所以链接器里才能些`ENTRY(_start)`

- `_start:` 汇编标签
- `la sp,boot_stack_top` 将栈顶符号`boot_stack_op`地址加载到栈寄存器`sp`
- `call rust_main` 调用rust的函数 rust_main
- `.section .bss.stack` 定义一个名为.bss.stack的段 对应链接器的`*(.bss.stack)`
- `.global boot_stack` 栈底符号
- `.space 4096*16`: 预留4096*16 即64kb的连续空间

这是内存布局的图

```
[ 低地址 ]
0x80200000 ->  +------------------+
               |  .text.entry     |  <- 执行 la sp, boot_stack_top
               +------------------+
               |  ...rust_main...   |
               +------------------+
               |  .bss.stack      |  <- boot_stack (栈底)
               |  (64KB 空间)      |       |
               |                  |       | 栈向下增长 (SP--)
               |                  |       v
               +------------------+
               |  boot_stack_top  |  <- 初始 SP 指向这里
[ 高地址 ]
```
#### 实现入口
现在可以导入刚才的汇编了 然后我们将入口改为rust_main

> 注意！#[no_mangle]必须添加 rust**默认会混淆函数名**。 否则汇编和链接器将无法正常看到rust_main的名字
```rust

core::arch::global_asm!(include_str!("entry.asm"));

#[no_mangle]
pub fn rust_main() -> ! {
	shutdown();
}
```

#### 清空.bss
在程序开始之前 我们应该先**清空.bss段**。 因为虽然很多操作系统会去清空 **但是我们最好不把这个作为信任前提** 所以我们手动清空bss

```rust
fn clear_bss() {
	extern "C" {
	 // 这里对应了.ld中的sbss和ebss 同时注意
	 // 函数指针默认指向的是第一个地址 所以使用函数指针可以指向sbss和ebss的头
	fn sbss();
	fn ebss();
}
(sbss as usize..ebss as usize).for_each(|a|
	{
	// 写0
	
	unsafe {(a as *mut u8).write_volatile(0)}
};
)
}
#[no_mangle]
pub fn  rust_main() -> ! {
clear_bss();
shutdown();
}
```

## 关联知识点
### 相对地址和绝对地址
#### 绝对地址
绝对地址是指程序指令中直接硬编码了具体的内存物理（或虚拟）地址。

链接阶段确定：当你设置 BASE_ADDRESS = 0x80200000 时，链接器会将所有全局符号（如 rust_main）绑定到基于此基址的固定位置。

指令表现：在 RISC-V 中，访问绝对地址通常需要两步，例如加载一个全局变量：

lui a0, %hi(sym) (加载符号的高 20 位)

ld a0, %lo(sym)(a0) (加载低 12 位并偏移)

硬核代价：如果程序被 QEMU 加载到了 0x90000000 而不是 0x80200000，所有基于绝对地址的跳转和内存访问都会指向错误的物理区域，导致 Load/Store Fault。

#### 相对地址
相对地址 (PC-Relative Address)
相对地址不关心当前在内存的哪个位置，它只关心“目标距离我有多远”。

指令表现：最典型的就是 jal (Jump and Link) 指令。其机器码中包含的是一个 Immediate Offset。

PC_new = PC_current + offset

工程优势：

位置无关性 (PIE)：如果你的整个 entry.asm 都使用相对跳转且不访问绝对地址符号，那么这段代码可以被放置在内存任何位置运行。

I-Cache 友好：相对跳转指令通常更短（如 c.j 压缩指令仅 2 字节），能显著提高指令缓存的密度和命中率。
## 示例问题
### 问题 1：为什么在 entry.asm 中必须先设置 sp 才能 call rust_main？Rust 的 panic_handler 在这种环境下又是如何找到栈的？

栈的必要性：Rust 编译后的函数（即使是 no_mangle 的 rust_main）在生成汇编时，通常会包含函数的 Prologue（开场白），用于保存返回地址 ra 和帧指针 s0/fp。如果 sp（栈指针）是随机值，这些写内存操作会导致地址访问违规（Load/Store Fault）。

Panic 机制：当代码触发 panic! 时，Rust 需要在栈上记录回溯信息（Backtrace）。在 nostd 裸机环境下，我们没有操作系统的信号处理。如果栈没初始化好就发生了 panic，CPU 会陷入死循环或触发非法的异常嵌套。

硬核细节：在 RISC-V 中，sp 寄存器必须保持 16 字节对齐。如果你在汇编里手动操作 sp 而未对齐，某些涉及 fld/fsd（浮点指令）的操作会直接触发硬件异常。

### 问题 2：你在链接脚本里用了 . = ALIGN(4K);。从 CPU 缓存（Cache）和页表（Page Table）的角度看，这种对齐的工程意义是什么？

内存保护（PMP/MMU）：RISC-V 的物理内存保护（PMP）或未来的页表（SV39/SV48）是以页为最小单位的。.text（只读/执行）、.rodata（只读）和 .data（读写）属性完全不同。如果不进行 4K 对齐，一个页里可能既包含代码又包含数据。为了安全，你无法为这个页设置纯“只读”或纯“不可执行”属性。

缓存命中优化：通过对齐，你可以确保内核的关键数据结构不会跨越两个不同的 Cache Line 或 TLB Entry。

i-Cache / d-Cache 分离：代码段和数据段在物理上分离开，有助于 CPU 更有效地预取指令，减少指令缓存和数据缓存之间的冲突挤占。

### 问题3: 在 clear_bss 函数中，你使用了 write_volatile。如果这里漏掉了 volatile 且开启了 cargo build --release，最坏的情况是什么？

编译器“幻觉”：Rust 编译器（LLVM 后端）在进行 O3 优化时，会分析代码的语义。如果它发现你只是在循环写 0 到一个后续“看起来没被用到”的内存区域，它可能会认为这是 Dead Store，从而直接把整个循环的代码删掉。

后果：由于 .bss 段包含未初始化的全局变量（在 Rust 中默认为 0），如果清空操作被优化掉，这些变量将包含加载时的随机物理内存残余。

对底层的影响：如果你的内核里有一个全局的 SpinLock 状态位存在 .bss 段，而它恰好因为没清零而处于非零值，你的内核会在第一次尝试获取锁时发生死锁，且这种 bug 极难调试。
