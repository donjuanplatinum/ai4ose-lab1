# Doom 移植与 Debug 涉及修改的文件和模块清单

在这个庞大的将 Doom 移植到 rCore/tg 操作系统（第八章，具备基于进程的文件描述符与条件变量）以及修复大量底层报错（包括内存泄漏、死锁、界面花屏和最终的 `LoadPageFault` 空指针解引用）的工程中，我们对整个操作系统的**内核层 (Kernel)** 和**用户态支持库 (User Space)** 进行了大量修改。

以下是具体的模块和文件修改清单及核心修改意图：

## 1. 内核空间 (Kernel - `tg-ch8`)

### `src/main.rs` (操作系统入口与系统调用分发)
- **VirtIO 设备发现与初始化**：在此处扫描并初始化了 `VirtIO-GPU`（显卡）和两台 `VirtIO-Input`（键盘与鼠标）设备，为后续的交互奠定物理驱动基础。
- **系统调用安全隔离重构 (`sys_read` / `sys_write`)**：
  - 这个文件历经了重大改造。为了彻底解决 Doom 引擎在传递不连续（跨页）虚拟内存缓冲区时触发的早期 `LoadPageFault`，我们利用 `AddressSpace::translate` 实现了**按页翻译 (Page-by-Page Translation)**。
  - 拦截了系统对特定路径 `/dev/gpu` (fd=4) 和 `/dev/input` (fd=3) 的请求，分别代理为向显示缓冲区（Framebuffer）批量写入和通过不断轮询拉取当前键盘按键状态队列 (`KEY_STATES`) 的操作。
- **并发锁 Bug 修复 (`condvar_wait`)**：重构了触发 `Option::unwrap()` Panic 的条件变量内核逻辑，移除了与 tg-sync 简化模型不兼容的 `wait_queue` 无脑塞入操作，严格管控互斥锁解锁后再立刻上锁的流程，拯救了跑飞死锁的 `test_condvar` 多线程测例。

### `src/process.rs` (基于进程的内存和资源管理)
- **进程销毁与内存回收 (`Drop` 接口)**：手动实现了 `impl Drop for Process`。当每次进程（比如 fork 出来的炸药包 `12forktest` 的子进程）退出时，遍历并翻译所有申请的地址段，最后调用 `alloc::alloc::dealloc` 还给物理分配器。修复了之前恐怖的内存泄漏（Out-of-Memory, OOM）问题。
- **内存所有权转移修复 (`exec` 函数)**：用 `core::mem::swap` 替代简单的赋值移动，顺利度过了新引入 `Drop` 后的 Rust 编译借用检查。

### `src/fs.rs` (文件系统抽象)
- **文件描述符的多态化 (`Fd` 枚举扩展)**：在底层的 `Fd` (File Descriptor) 枚举类型中新增了两个变体——`VirtioGpu` 和 `VirtioInput`。这允许像 `pipetest` 这类需要正常复用 3、4号 FD（用于管道通信）的用户程序和 Doom 这种强依赖专属设备的程序能够和平共处。

### `Cargo.toml`
- 引入了官方的 `virtio-drivers` 依赖，允许系统以 MMIO 的方式驱动 QEMU 挂载的外设。

---

## 2. 用户态交叉编译 C 库 (User Space - `doomgeneric`)

Doom 是用古老的 C 语言编写的，它非常依赖操作系统的 C 标准库来读写文件、分配内存并管理屏幕。我们完全“手搓”实现了适配这些接口的支持层：

### `doomgeneric_tgos.c` (操作系统平台接口 Backend)
- **核心生命周期控制**：实现了 Doom 渲染和输入循环要求的 6 大基础方法 (`DG_Init`, `DG_DrawFrame`, `DG_SleepMs`, `DG_GetTicksMs`, `DG_GetKey`, `DG_SetWindowTitle`)。
- **键码转换引擎 (`scancodeToDoom`)**：写了一个长长的 switch 映射，将底层的 Linux/QEMU evdev 扫描码翻译为内部所识别的 Doom 专用键值（如把空格映射为开火 `KEY_FIRE`/`KEY_USE`，方向键映射为跑动）。
- **设备文件绑定**：在此处直接发起 `sys_open("/dev/input")` 拿到键盘专属描述符；`sys_open("/dev/gpu", O_WRONLY)` 拿到屏幕描述符，之后每次 `DG_DrawFrame` 就向该 fd 无脑爆满一整个 640x400 的 rgba 缓存。

### `libc_shim.c` (迷你自定义 C 标准库 - 🌟 Bug 重灾区)
这个超过 600 行的文件为 Doom 模拟了一套 Linux/Posix 环境。
- **堆内存大内管家 (`malloc` / `realloc` / `free`)**：
  - 基于一块写死的 32MB 静态内存区 `static char _heap[32*1024*1024]` 实现了一个极简的 Bump Allocator（只借不还的分配器）。因为 Doom 初期内存膨胀高达 8MB 以上，如果没有它会导致游戏直接 OOM 坠机。
- **文件与 WAD 引擎缓存 (`fopen` / `fread` / `fseek`)**：
  - 把 C 的文件句柄直接桥接到系统的 syscall (`fd`)。
  - **核心加速手段（Wad Caching）**：在遇到打开 `.wad` 游戏资源包时，与其忍受引擎反复进行 `sys_read` 然后不断 `fseek` 到处乱跳产生的卡顿现象，干脆设计并在内存中直接 `malloc` 了一片 6MB 的连续数组，把整个原始的 `doom1.wad` 一次性吃进内存，实现 O(1) 的超高速寻址！
- **字符串与格式化处理 (`strcmp` / `snprintf` 等)**：
  - 包括 `memset`、`memcpy`、`strcmp` 等基础函数。
  - **最后一修（0x30e18 致命空指针案）**：这就是最终困扰我们的地方——`DEH_snprintf` 使用简化的 `_vformat` 时抛弃了数字的前导补零精度约束。最终通过修补那五行通过判断 `have_prec` 如果命中精度则要求强制 `zero = 1;` 触发使用 `'0'`（而非空格）占位符填充宽度的逻辑，让 `"STCFN%.3d"` 拼出了正确的 `"STCFN033"`，避免了因找不到 Lump 导致数组越狱拿到 Null 渲染引擎崩溃的情况。

---

## 3. 辅助打包与运行工具 (Tooling)

### `Makefile.tgos`
我们不再使用默认 Linux 工具链，而是手写了一个仅针对裸核心平台的 `riscv64-unknown-none-elf` Makefile，强制编译并将这些模块用静态链接 (Static Linking) 合成最终那颗孤独而倔强的单体二进制 `doom`。

### `run_doom2.py`
为了绕开纯手打，利用 Python 的 `pexpect` 库构建了一套终端劫持小脚本，自动开机 -> 等待 `Rust user shell` -> 输入 `doom` 命令 -> 然后把日志和输出完好地保留到本地 `doom.log` 用于进行反编译比对和崩溃点追踪。
