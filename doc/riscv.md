# RISC-V 系统编程硬核知识点

## 1. 基础汇编 (Essential Assembly)

在内核开发中，除了标准的算术逻辑指令，以下指令是构建内核骨架的核心：

| 指令 | 全称 | 作用 | 内核应用场景 |
| :--- | :--- | :--- | :--- |
| `la rd, symbol` | Load Address | 加载符号地址 | 获取全局变量、跳转入口的绝对/相对地址 |
| `auipc rd, imm` | Add Upper Immediate to PC | `rd = PC + (imm << 12)` | 实现 PC 相关（Position Independent）的代码寻址 |
| `jalr rd, offset(rs1)` | Jump and Link Register | 跳转并保存返回地址 | 配合 `auipc` 实现远距离跨度（±2GB）的函数调用 |
| `ecall` | Environment Call | 触发异常进入更高特权级 | 用户态请求内核态，或内核态请求 SBI (M-Mode) |
| `mret / sret` | Machine/Supervisor Ret | 从异常处理程序返回 | 实现特权级切换的关键：从 M 回到 S，或从 S 回到 U |
| `csrrw rd, csr, rs1` | CSR Read Write | 交换寄存器与 CSR 的值 | 读写 `satp` (页表)、`stvec` (中断向量表) 等 |

---

## 2. 运行权级 (Privilege Levels)

RISC-V 的特权级设计简洁，但控制极其严格。

* **U (User) Mode**: 用户态，受限访问。
* **S (Supervisor) Mode**: 内核态。我们的 rCore/Rust 内核运行在此级别。拥有访问 MMU、部分 CSR 的权限。
* **M (Machine) Mode**: 最高特权级。RustSBI 运行在此。对硬件有绝对控制权。
* **特权级转换**: 
    - **向上**: 只能通过异常（Exception/Interrupt），如 `ecall`、外部中断。
    - **向下**: 只能通过修改 `mstatus.MPP` 或 `sstatus.SPP` 字段后执行 `mret` 或 `sret`。

---

## 3. 通用寄存器 (Registers - ABI View)

作为优化控，你需要精确控制寄存器分配，尤其是 `sp` 的对齐和 `tp` 的使用。

| 寄存器 | ABI 名称 | 描述 | 保存者 (Saver) |
| :--- | :--- | :--- | :--- |
| **x0** | **zero** | 永远为 0 | —— |
| **x1** | **ra** | 返回地址 (Return Address) | Caller |
| **x2** | **sp** | 栈指针 (Stack Pointer) | Callee (16字节对齐) |
| **x4** | **tp** | 线程指针 (Thread Pointer) | 在多核内核中常用于存放 CPU 核心结构体指针 |
| **x8** | **s0/fp** | 帧指针 (Frame Pointer) | Callee |
| **x10-x11**| **a0-a1** | 函数参数 / 返回值 | Caller |
| **x17** | **a7** | SBI / Syscall 调用号 | Caller |

---

## 4. 启动流程 (Boot Flow & Addresses)

在 QEMU `virt` 平台下，启动是一个精密的接力过程。

### 关键内存地址
* **0x00001000**: QEMU Vmask ROM 入口。CPU 加电后的第一条指令位置。
* **0x80000000**: **RustSBI 入口地址**。DRAM 的起始位置，QEMU 完成跳转。
* **0x80200000**: **Kernel 入口地址**。RustSBI 完成初始化后，会跳转到此地址将控制权移交给我们的 S-Mode 内核。



### 启动链条细节
1.  **硬件初始化**: CPU 重置，PC 设为 `0x1000`。
2.  **M-Mode (RustSBI)**: 
    - 位于 `0x80000000`。
    - 探测硬件信息，填充 DTB（设备树）。
    - 设置中断委托（Delegation），将部分中断交给 S-Mode 处理。
    - 执行 `mret` 降权跳转到 `0x80200000`。
3.  **S-Mode (Your Kernel)**:
    - 执行 `_start` (entry.asm)。
    - **设置 `sp`**: 初始化内核栈空间。
    - **清空 `.bss`**: 保证 Rust 全局变量的确定性。
    - **跳转 `rust_main`**: 进入 Rust 世界。

### 系统调用入口 (S-Mode)
* 内核通过 `stvec` 寄存器设置中断处理程序的绝对地址。
* 当用户态触发 `ecall`，PC 跳转至 `stvec` 所指地址。
