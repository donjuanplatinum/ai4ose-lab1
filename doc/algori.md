# 🛠️ 虚拟存储：MMU 与分页机制知识图谱

## 1. 核心机制：MMU (Memory Management Unit)
MMU 是存在于 CPU 内部的硬件单元，其核心职责是将虚拟地址（VA）在指令执行瞬间翻译为物理地址（PA）。

### 硬件机制流 (Hardware Logic)
* **地址翻译流**：VA -> TLB 检索 -> (Miss) -> 硬件页表遍历 (Page Walk) -> PA。
* **权限校验**：在翻译的同时，MMU 检查 PTE 中的 `R/W/X/U` 标志位。
* **CSR 寄存器控制 (RISC-V)**：
    * `satp` (Supervisor Address Translation and Protection)：
        * `MODE`: 开启模式（如 Sv39, Sv48）。
        * `ASID`: 地址空间标识符，用于优化 TLB 刷新频率。
        * `PPN`: 根页表的物理页号。

## 2. 核心数据结构：分页机制 (Paging)
分页是将物理内存裁切成固定大小的“页帧”（Page Frame），并通过树状结构进行索引的策略。

### 分页层级 (以 Sv39 为例)
* **三级页表结构**：39 位 VA 分为 $L2(9) | L1(9) | L0(9) | Offset(12)$。
    * **PTE (Page Table Entry)**：
        * `PPN`: 物理页号。
        * `Flags`: `V`(有效), `R/W/X`(权限), `U`(用户可访问), `G`(全局), `A`(已访问), `D`(脏页)。
* **页表项对齐**：每个 PTE 占据 8 字节，一个 4KB 的页刚好容纳 512 个 PTE（即 $2^9$）。

---

## 3. 硬核优化算法 (Optimization Algorithms)

### A. 缓存与 TLB 优化
* **TLB ASID 策略**：上下文切换时，通过 ASID 区分不同进程的 TLB 项，避免执行高开销的 `sfence.vma`（全量缓存刷新）。
* **大页优化 (Huge Pages)**：
    * **机制**：在 L1 或 L2 层级直接终止 Page Walk，映射 2MB 或 1GB 连续物理空间。
    * **目的**：大幅减少 TLB Miss，降低页表遍历的访存深度。

### B. 页面置换算法 (Page Replacement)
当物理内存不足（Memory Overcommit）时，决定“牺牲”哪个物理页：
* **Clock 算法 (时钟算法)**：利用 PTE 的 `A` 位，以较低的 O(1) 复杂度模拟 LRU 效果。
* **Working Set 算法**：基于局部性原理，动态维护进程频繁访问的页面集合，减少抖动（Thrashing）。

### C. 内存分配算法 (Allocation)
* **伙伴系统 (Buddy System)**：
    * **目标**：解决外部碎片。
    * **逻辑**：将内存按 $2^n$ 分级，通过合并与拆分管理连续物理页。
* **SLUB 分配器**：
    * **目标**：解决内部碎片。
    * **逻辑**：针对内核高频小对象（如 `TaskStruct`, `File`）建立 Cache 缓存池，提升 Cache Line 命中率。

---

## 4. 算法与机制关系对照表

| 维度 | 硬件机制 (Mechanism) | 软件算法/策略 (Policy) | 工程优化手段 |
| :--- | :--- | :--- | :--- |
| **地址翻译** | 多级页表步进 (Page Walk) | 缺页异常处理 (Page Fault) | **Lazy Allocation (延迟分配)** |
| **缓存管理** | TLB 自动加载 | 缓存一致性维护 (fence.vma) | **ASID 染色 (Coloring)** |
| **空间保护** | PTE 标志位校验 | 内存映射管理 (VMA Tree) | **Copy-on-Write (写时复制)** |
| **物理管理** | MMU 访存 | 伙伴系统 / SLUB | **Page Coloring (减少 Cache 冲突)** |

---

## 5. 进阶：内核工程中的“页优化”
作为 Rust 内核工程师，以下是你必须关注的底层细节：

1.  **指令缓存一致性 (I-Cache Consistency)**：
    * 搬运代码到新页后，必须执行 `fence.i`。
2.  **内核栈保护**：
    * 在内核栈底设置一个不映射任何 PA 的 **Guard Page**。当发生栈溢出时，MMU 会立即触发异常，防止破坏其他内核数据。
3.  **零拷贝 (Zero-copy)**：
    * 通过重映射（Re-mapping）PA 到不同的 VA，实现数据在用户态和内核态之间的“瞬间转移”，消除 `memcpy` 开销。
