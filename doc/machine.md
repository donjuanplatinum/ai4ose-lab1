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
