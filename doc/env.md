# 环境配置
一直以来 操作系统开发的环境配置相对复杂 涉及到交叉编译,qemu转译,平台限制 等诸多问题。 有时候甚至会因为distro的原因 或者qemu版本的原因导致各种bug。

为了解决配环境的问题 减少在环境上的时间 更快进入操作系统真正内容的学习 我们使用nix包管理器进行**原子级别**的环境配置.

对于Nix管理器 在这里您只用知道的是 `Nix 是一个基于函数式范式的声明式配置工具 你的环境具有原子性 函数的输入是怎样 环境就是固定的`

可以查阅[NixOS](https://nixos.org)来了解更多.

AI助手 本章思维导图:
```
mindmap
  root((AI4OSE 环境配置))
    Nix 原子化部署
      原理: 函数式输入 = 固定环境输出
      工具: Nix Flakes
      优势
        消除 Distro 差异
        一键部署交叉编译工具链
        版本锁定: QEMU 7.0 / Rust Nightly 2024-05-02
    核心工具链
      Rust
        Target: riscv64gc-unknown-none-elf
        Component: rust-src, llvm-tools-preview
      QEMU 7.0
        System: qemu-system-riscv64
        User: qemu-riscv64
      Debugger: GDB + Python 脚本
    rCore-Tutorial 运行
      源码控制: git checkout ch1
      底层接口: RustSBI (Supervisor Binary Interface)
      特权级切换: User -> Supervisor (Kernel)
    AI 协同学习 (Active Recall)
      原子性理解: 一致性 / 回滚 / CAS 指令
      QEMU 作用: 快速迭代 / GDB Stub 调试 / 硬件模拟
      性能分析: 缓存命中率 / 系统调用开销 (Bench 集成预留)
```

## 安装nix与配置环境
### Nix安装
为了使用nix包管理器 我们需要安装nix(当然您也可以安装NixOS系统).

```shell
sh <(curl --proto '=https' --tlsv1.2 -L https://nixos.org/nix/install) --no-daemon
```

### flake.nix
这是我们项目的flake.nix
```nix
{
  description = "AI4OSE OS Kernel Development Environment";

  inputs = {
    nixpkgs-qemu.url = "github:nixos/nixpkgs/nixos-22.05";
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, nixpkgs-qemu, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        pkgs-old = import nixpkgs-qemu { inherit system; };

        rustToolchain = pkgs.rust-bin.nightly."2024-05-02".default.override {
          targets = [ "riscv64gc-unknown-none-elf" ];
          extensions = [ "rust-src" "llvm-tools-preview" "rust-analyzer" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            pkgs.cargo-binutils
            
            pkgs-old.qemu

            pkgs.gdb
            pkgs.python3
            pkgs.gnumake
            pkgs.dtc
          ];

          shellHook = ''
            echo "--- AI4OSE Environment Loaded ---"
            echo "Rust: $(rustc --version)"
            echo "QEMU: $(qemu-system-riscv64 --version | head -n 1)"
            echo "Target: riscv64gc-unknown-none-elf"
          '';
        };
      }
    );
}

```
### flake环境部署
安装完nix后 执行这条命令
```shell
nix  --extra-experimental-features 'nix-command flakes' develop --option substituters "https://mirrors.tuna.tsinghua.edu.cn/nix-channels/store"
```

这条命令会根据仓库的flake.nix来构建环境,请注意 因为是从源代码进行的构建的 所以需要等待一段时间.

您会得到类似的输出 
```shell
warning: file 'nar/0214x8na3jxnc9d8xyyrmh7rgbk3sigi93pwjvb5kgg9mi7xxv7x.nar.xz' does not exist in binary cache 'https://mirrors.tuna.tsinghua.edu.cn/nix-channels/store'
[1/10/378 built, 13.4/13.8 MiB DL] downloading 'https://github.com/nghttp2/nghttp2/releases/download/v1.47.0/nght
```

> 请注意 不用担心这里的warning 它指的是服务器没有缓存原子相等的二进制包 所以需要从源码进行构建

### 检验安装
Rust工具链应为2024-05-01的nightly 且target应包含x86_64-unknown-linux-gnu

```shell
rustc --version
rustc 1.80.0-nightly (c987ad527 2024-05-01)
```

```shell
rustup target list |grep -i installed
x86_64-unknown-linux-gnu (installed)
```

qemu应该安装了riscv-64相应的system和user的工具链 版本应为7.0.0
```shell
qemu-riscv64 --version
qemu-riscv64 version 7.0.0
Copyright (c) 2003-2022 Fabrice Bellard and the QEMU Project developers
```

```shell
qemu-system-riscv64 --version
QEMU emulator version 7.0.0
Copyright (c) 2003-2022 Fabrice Bellard and the QEMU Project developers
```
## 实验1 编写一个可发布到crates.io的最简单rust程序
## 运行rCore
```shell
git clone https://github.com/LearningOS/rCore-Tutorial-Code-2025S
cd rCore-Tutorial-Code-2025S
git checkout ch1
cd os
LOG=DEBUG make run
```

接下来您会得到如下输出
```shell
[rustsbi] RustSBI version 0.3.0-alpha.4, adapting to RISC-V SBI v1.0.0
.______       __    __      _______.___________.  _______..______   __
|   _  \     |  |  |  |    /       |           | /       ||   _  \ |  |
|  |_)  |    |  |  |  |   |   (----`---|  |----`|   (----`|  |_)  ||  |
|      /     |  |  |  |    \   \       |  |      \   \    |   _  < |  |
|  |\  \----.|  `--'  |.----)   |      |  |  .----)   |   |  |_)  ||  |
| _| `._____| \______/ |_______/       |__|  |_______/    |______/ |__|
[rustsbi] Implementation     : RustSBI-QEMU Version 0.2.0-alpha.2
[rustsbi] Platform Name      : riscv-virtio,qemu
[rustsbi] Platform SMP       : 1
[rustsbi] Platform Memory    : 0x80000000..0x88000000
[rustsbi] Boot HART          : 0
[rustsbi] Device Tree Region : 0x87000000..0x87000ef2
[rustsbi] Firmware Address   : 0x80000000
[rustsbi] Supervisor Address : 0x80200000
[rustsbi] pmp01: 0x00000000..0x80000000 (-wr)
[rustsbi] pmp02: 0x80000000..0x80200000 (---)
[rustsbi] pmp03: 0x80200000..0x88000000 (xwr)
[rustsbi] pmp04: 0x88000000..0x00000000 (-wr)
[kernel] Hello, world!
```

## AI助手提问

### 环境
#### 示例问题
点击三角符号展开

<details>
<summary><b>什么是原子性? 原子性有哪些好处?为什么NixOS要选择使用原子性的特性?原子性在CPU底层有哪些用处?</b></summary>

1. 原子性的好处

从工程和算法优化角度看，原子性带来了以下核心优势：

数据一致性（Consistency）： 消除竞态条件（Race Conditions）。在多核并行时，确保观测到的数据永远是合法的。

异常安全性（Exception Safety）： 如果操作中途崩溃或掉电，系统由于不存在“半完成”状态，可以回滚或保持原样，避免产生脏数据（Dirty data）。

无锁化设计（Lock-free）： 配合 CAS（Compare-and-Swap）等指令，可以避免昂贵的互斥锁（Mutex）开销，减少上下文切换和内核态转换。

2. NixOS 为何选择原子性？

NixOS 走向了另一个极端：声明式与不可变性。

NixOS 的“原子性升级”基于 symlink 的原子替换。其核心逻辑如下：

原子切换（Atomic Switching）： 所有的软件包和配置文件都存储在 /nix/store/ 下唯一的 Hash 路径中。当你切换系统配置时，Nix 只是简单地修改 /run/current-system 这个软链接指向新的路径。

底层行为： 这利用了 Linux 内核 renameat2 或简单的 symlink 原子覆盖，确保在任何时刻，系统文件目录要么指向旧版本，要么指向新版本。

无损回滚（Rollback）： 如果新配置导致 Kernel Panic，引导加载程序可以原子性地指回旧的生成版本（Generation）。

消除副作用： 传统的 emerge 或 pacman 是就地更新（In-place update），如果进程中途被杀掉，系统可能处于不一致的损毁状态。NixOS 的原子性彻底杜绝了“半更新”导致的系统崩溃。

3. 原子性在 CPU 底层的用处

作为 Rust 内核工程师，你对这一层肯定最感兴趣。在 CPU 硬件层面，原子性是维护 L1/L2 Cache 一致性和**内存序（Memory Ordering）**的关键。

核心机制：总线锁与缓存锁定
Total Store Ordering (TSO): 在 x86_64 架构下，硬件提供了较强的原子性保障。当执行带 LOCK 前缀的指令（如 LOCK XCHG）时，CPU 会通过 MESI 协议 锁定特定的缓存行（Cache Line），防止其他核心在操作期间修改该内存地址。

Read-Modify-Write (RMW): CPU 底层通过原子操作实现 RMW。如果没有硬件级的原子性，一个简单的 i++ 会被分解为 MOV, ADD, MOV 三条指令，中间极易被中断或核间干扰。

</details>

<details>
<summary><b>Qemu是什么?Qemu在开发Linux内核时起到什么作用?</b></summary>

1. 什么是 QEMU？
QEMU（Quick Emulator）是一个开源的托管虚拟机监控程序（VMM）。它的核心能力在于：

二进制翻译（TCB）： 能够跨架构模拟，比如在 x86_64 宿主机上运行 ARM 或 RISC-V 的内核。

硬件仿真： 模拟 CPU、内存、中断控制器（APIC/NVIC）、物理定时器以及各类总线设备（PCIe, VirtIO）。

KVM 配合： 在 Linux 下，QEMU 可以利用 /dev/kvm 实现硬件加速，让 Guest OS 直接在物理 CPU 上执行非敏感指令，接近原生性能。

2. QEMU 在 Linux 内核开发中的核心作用
A. 极其高效的“编译-运行-测试”循环
在真机上调试内核需要反复烧录存储介质、等待 BIOS/UEFI 引导。

直接加载内核： 你可以使用 -kernel 参数直接引导编译好的 bzImage 或 vmlinux，跳过复杂的 Bootloader 配置。

Initrd 注入： 通过 -initrd 挂载根文件系统，配合内存文件系统极速启动。

B. 源码级远程调试（GDB Stub）
这是 QEMU 区别于物理机最强的优势。通过 -s -S 参数，QEMU 会在启动时挂起，并开启一个 GDB Server（默认 1234 端口）。

非侵入性调试： 你可以像调试用户态程序一样，在内核启动的第一行代码（甚至是实模式到长模式的切换点）打断点。

查看底层寄存器： info registers 或查看控制寄存器（CR0, CR3, CR4），这在真机上很难实时捕捉。

C. 验证底层优化与内存对齐
由于你关注 CPU 缓存和页优化：

内存布局模拟： 可以通过 -m 和特定的 NUMA 配置，测试内核在复杂内存拓扑下的调度表现。

页表跟踪： QEMU 的监控控制台（Monitor）允许你直接查看 Guest 的物理内存和虚实映射关系（xp 指令）。

D. 模拟特定的硬件故障或边界条件
多核压力测试： 通过 -smp 模拟多核竞态，验证你 Rust 代码中的原子操作和 spin_lock 是否真的能在高并发下保持一致性。

中断处理： 精确控制外设中断的触发，调试中断处理函数（ISR）的原子性逻辑。

</details>
