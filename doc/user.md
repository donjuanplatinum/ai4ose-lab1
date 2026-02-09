# 用户态应用程序

![UNIX框架](../resource/arch_of_unix.png)

大多数的时候我们一直以来接触的都是**用户态**的应用程序，小到HelloWorld 大到Emacs，LLM框架，各种游戏，它们都存在于UNIX架构的最外层 即用户态。

本章我们会逐步实现用户态以下的**内核程序** 然后使得我们的用户态程序可以运行于之上。


目前我们只使用Rust的**core**库 而不使用rust的*std**，因为std需要**完整的操作系统交互支持**。 我们先来介绍Rust的语言架构 然后会介绍编译器的流程

## Rust架构
![Rust架构](../resource/rust_dep.png)

Rust的语言分为4个库： `alloc`,`core`,`proc_macro`,`std`. 它们构成了Rust的灵魂。

其中 `core`库是Rust的骨架，它是Rust`最基本` `最通用` `最底层`的库。 基本上无论什么环境 只要有Rust编译器 就能使用core库。

而`alloc`库掌管了Rust的`内存分配`领域，是Rust迈向与操作系统交互，堆分配的重要基石。

`std`是Rust的标准库 由`core`+`alloc`+`操作系统C库(libc)`实现

>我们编写操作系统内核只能使用no_std环境 因为内核在初始化初期时 内存管理器是无法使用的 所以我们只能使用core库

## 编译器流程
### 编译与链接
rust的代码属于**伪代码**

而计算机真正能读取的是**二进制**指令集

编译器的功能简单来说分为两步: 1. 将伪代码编译为汇编 2. 将汇编进行静态链接或动态链接

而在Rust里 第一步会增加一个翻译为LLVM的过程。

即`Rust -> LLVM IR -> ASM`


>为什么很多人一直说Rust的**体积都很大**呢？ 

实际上是因为Rust默认为**静态链接**。  静态链接会把**所有的库**全部编译到一个二进制文件里。

>那么为什么Rust不使用**动态链接**呢？ 

因为Rust是一门一直处于**发展阶段**的语言 固然它的ABI是一直在发生变化的。 如果ABI发生变化 那么库的二进制也会发生改变。 只有当Rust的ABI定下来的那一天 才会达到/usr/lib下都是Rust的lib的盛世。

### 汇编与平台
CPU架构: 汇编语言是与平台**强相关**的 AMD64与RISCV64的汇编是完全不一样的 它们有着不同的**寄存器** 不同的**指令集** 不同的**特权状态**

操作系统: 不同的**操作系统的API**是不一样的。 UNIX有着UNIX的标准系统调用，Linux有Linux特有的系统调用，Windows也有自己的系统调用 叫ntdll

动态链接C库： 有的运行时库是glibc 有的是clang 它们在很多特性上不一样。

**目标三元组**描述了这些特征




## 一个简单的no_std rust程序
在os目录下新建.cargo目录，并在config文件里指定平台以进行**交叉编译**

> 我们使用了自定义的panic_handler 因为默认的panic操作需要std

> 我们并没有定义main函数入口 因为默认的main函数入口需要`_start()` 而目前的环境并不支持
```toml
# os/.cargo/config
[build]
target = "riscv64gc-unknown-none-elf"
```

```rust
#![no_std]
#![no_main]

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
	loop{}
}
```

### 分析
file命令可以查看文件的类型

```shell
ELF 64-bit LSB executable, UCB RISC-V, RVC, double-float ABI, version 1 (SYSV), statically linked, with debug_info, not stripped
```

rust-readobj命令可以用于读取二进制ELF文件的具体信息

```shell
File: ./target/riscv64gc-unknown-none-elf/debug/rust-bench
Format: elf64-littleriscv
Arch: riscv64
AddressSize: 64bit
```


## 构建用户态的执行环境
在上一章中我们定义了
