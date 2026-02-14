# RISC-V 系统编程硬核知识点

## 1. 基础汇编 (Essential Assembly)

在内核开发中，除了标准的算术逻辑指令，以下指令是构建内核骨架的核心：

| 指令                   | 全称                      | 作用                    | 内核应用场景                                     |
|:-----------------------|:--------------------------|:------------------------|:-------------------------------------------------|
| `la rd, symbol`        | Load Address              | 加载符号地址            | 获取全局变量、跳转入口的绝对/相对地址            |
| `auipc rd, imm`        | Add Upper Immediate to PC | `rd = PC + (imm << 12)` | 实现 PC 相关（Position Independent）的代码寻址   |
| `jalr rd, offset(rs1)` | Jump and Link Register    | 跳转并保存返回地址      | 配合 `auipc` 实现远距离跨度（±2GB）的函数调用   |
| `ecall`                | Environment Call          | 触发异常进入更高特权级  | 用户态请求内核态，或内核态请求 SBI (M-Mode)      |
| `mret / sret`          | Machine/Supervisor Ret    | 从异常处理程序返回      | 实现特权级切换的关键：从 M 回到 S，或从 S 回到 U |
| `csrrw rd, csr, rs1`   | CSR Read Write            | 交换寄存器与 CSR 的值   | 读写 `satp` (页表)、`stvec` (中断向量表) 等      |

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

## 5. 核心控制与状态寄存器 (CSRs - Supervisor Mode)

在 S-Mode 内核开发中，CSR 是控制硬件行为的“方向盘”。RISC-V 规定了专门的指令来操作它们：`csrr` (读)、`csrw` (写)、`csrrw` (读写交换)、`csrrs` (置位)、`csrrc` (清零)。

### 5.1 状态与异常控制寄存器
| CSR 名称 | 全称 | 作用与硬核细节 |
| :--- | :--- | :--- |
| **`sstatus`** | Supervisor Status | **核心状态寄存器**。控制全局中断使能（SIE）以及 `sret` 指令的行为。其中的 `SPP` 位决定了 `sret` 后回到哪个特权级，`SPIE` 记录了触发异常前中断是否开启。 |
| **`stvec`** | Supervisor Trap Vector | **异常入口基址**。存储 Trap 处理程序的入口地址。支持两种模式：Direct（所有异常跳转到同一地址）或 Vectored（按异常原因跳转到不同地址）。 |
| **`sepc`** | Supervisor Exception PC | **异常返回地址**。当 Trap 发生时，硬件自动将当前指令 PC 存入此处。`sret` 指令会读取此值并跳转回原来的执行流。 |
| **`scause`** | Supervisor Cause | **异常原因**。记录 Trap 是因为系统调用 (`Environment Call from U-mode`)、外部中断还是非法指令等。最高位（Interrupt 位）用于区分中断和异常。 |
| **`stval`** | Supervisor Trap Value | **异常附加信息**。如果是访存异常，此处记录出错的虚拟地址；如果是指令异常，此处可能记录指令码。 |



### 5.2 内存管理与地址转换
| CSR 名称   | 全称                                          | 作用与内核应用                                                                                                                                                                   |
|:-----------|:----------------------------------------------|:---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **`satp`** | Supervisor Address Translation and Protection | **页表控制寄存器**。控制 MMU 开启。包含 `MODE`（如 SV39）、`ASID`（地址空间标识符，减少 TLB 刷新）和 `PPN`（根页表的物理页号）。**写入此寄存器通常需要紧跟 `sfence.vma` 指令。** |
|            |                                               |                                                                                                                      |

### 5.3 优化与上下文切换神器
| CSR 名称 | 全称 | 极底层优化场景 |
| :--- | :--- | :--- |
| **`sscratch`** | Supervisor Scratch | **上下文切换的桥梁**。通常在 U 态运行时存放内核栈指针。当 Trap 发生，第一条指令往往是 `csrrw sp, sscratch, sp`，瞬间完成用户栈与内核栈的物理切换，且不破坏任何通用寄存器。 |

### 5.4 性能监控寄存器 (Performance Counters)
作为优化控，你可以直接在 S-Mode 读取以下只读寄存器进行 Profile：
* **`cycle`**: CPU 时钟周期计数器。
* **`time`**: 实时时间计数器（与硬件频率相关）。
* **`instret`**: 已完成执行的指令数量计数器。



---

### 工程师视角：CSR 操作的代价
1. **流水线停顿 (Pipeline Stall)**: 频繁读写 CSR 会强制同步流水线。尤其是 `satp` 的写入会触发 TLB 的潜在失效。
2. **原子性**: CSR 指令是单条原子操作。在 `nostd` 环境下，通过 `csrrs` 关闭全局中断是实现内核临界区最快的方式，开销远小于通过 Rust 标准库封装的锁。
3. **指令选型**: 在汇编层面，如果你只需要写而不需要读旧值，使用 `csrw` 比 `csrrw` 理论上对发射逻辑更友好。
