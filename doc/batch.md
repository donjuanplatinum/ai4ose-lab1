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
      SYSCALL_EXIT (93): 程序正常退出, 触发加载下一个 App```
