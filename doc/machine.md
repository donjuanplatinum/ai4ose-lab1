# 裸机执行环境
在本节中 我们将把`hello,wolrd`从**用户态** 搬到**内核态**

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

