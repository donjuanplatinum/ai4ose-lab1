# Doom 移植调试：修复 LoadPageFault (stval=0x3)

在成功将 Doom 移植到 tg-ch8 并在 QEMU 中启动后，我们遇到了一个致命的崩溃：

```text
[ERROR] unsupported trap: Exception(LoadPageFault)
[ERROR] stval = 0x3
[ERROR] sepc  = 0x30e18
Shell: Process 3 exited with code -3
```

本文档将详细记录针对这个 `LoadPageFault` 的完整调试与修复流程，展示如何从底层的硬件异常信息，一步步顺藤摸瓜，最终定位并修复 C 语言层面的标准库实现缺陷。

## 1. 异常定位与初步推理

当操作系统抛出 `Exception(LoadPageFault)` 时，意味着程序试图访问一个未映射或无权限的内存地址。

关键线索在于引发异常的具体寄存器状态：
*   **`sepc = 0x30e18`**：发生错误的指令地址 (PC 指针)。
*   **`stval = 0x3`**：导致缺页异常的目标内存地址。

> [!IMPORTANT]
> **推理:** `stval = 0x3` 接近 `0x0`（NULL）。在 99% 的情况下，这种极低的非法内存地址意味着**空指针解引用 (Null Pointer Dereference)**。程序很可能是拿到了一个空指针 (0x0)，然后试图访问偏移量为 3 的结构体成员（即 `0x0 + 3`）。这说明并非内核的 `sys_write/sys_read` 内存映射出错，而是 Doom 的用户态 C 代码逻辑崩溃。

## 2. 反汇编分析：寻找肇事指令

为了查明是哪段 C 代码造成了崩溃，我们使用 `rust-objdump` 对编译出的 Doom ELF 文件进行反汇编，目标地址设定在 `0x30e18` 附近。

运行命令：
```bash
rust-objdump -Sd --start-address=0x30df0 --stop-address=0x30e40 doom
```

反汇编结果中的关键片段如下：
```assembly
30e10: bb9ff0ef     jal     0x309c8 <HUlib_initSText>
30e14: 3e04b503     ld      a0, 0x3e0(s1)
30e18: 00350583     lb      a1, 0x3(a0)    <-- 崩溃发生在这里！
30e1c: 00254503     ld      a1, 0x3(a0)
30e20: 05a2         slli    a1, a1, 0x8
...
30e36: 959ff0ef     jal     0x3078e <HUlib_initTextLine>
```

> [!NOTE]
> **发现:** 崩溃发生在 `lb a1, 0x3(a0)`，这完美吻合了 `stval = 0x3`（尝试从 `a0 + 0x3` 读取一个字节）。通过查看上下文符号 `HUlib_initSText` 和 `HUlib_initTextLine`，可以确认这段代码属于 Doom 的 **HUD（抬头显示，Heads Up Display）文本初始化及渲染部分**。

## 3. 关联日志：定位逻辑错误点

既然确定了崩溃发生在 HUD 的初始化阶段，我们回头检查崩溃前 Doom 输出的运行日志：

```text
HU_Init: Setting up heads up display.
HU_Init: Lump STCFN33 NOT FOUND
HU_Init: Lump STCFN34 NOT FOUND
...
HU_Init: Lump STCFN95 NOT FOUND
I_InitGraphics: Auto-scaling factor: 2
```

日志显示，Doom 试图加载名为 `STCFNxx` 系列的 Lump（WAD 文件中的数据块，这里是 HUD 的英文字体贴图），但全部失败。

> [!CAUTION]
> 字体 Lump 加载失败后，数组中保存了空指针或无效数据。当后续 `HUlib_initTextLine` 被调用，准备渲染字符串时，它试图读取这些缺失字体的属性（例如宽度），从而触发了 `0x3` 的空指针解引用。

但是为什么会找不到字体？在原始的 `doom1.wad` 文件中，这些字体的正确命名应该是 `STCFN033` 到 `STCFN095`（带有前导零）。而日志里打印的却是 `STCFN33`。

## 4. 追溯源码：发现字符串格式化漏洞

为了弄清楚名字是如何生成的，在 Doom 源码中搜索 `STCFN`，在 [hu_stuff.c](file:///home/donjuan/git/ai4ose-lab1/doomgeneric/doomgeneric/hu_stuff.c) 中找到了关键的初始化逻辑：

```c
// hu_stuff.c (HU_Init 函数)
j = HU_FONTSTART;
for (i=0;i<HU_FONTSIZE;i++)
{
    DEH_snprintf(buffer, 9, "STCFN%.3d", j++);
    int lumpnum = W_CheckNumForName(buffer);
    // ...
}
```

这里使用了 `DEH_snprintf`（宏定义为 [snprintf](file:///home/donjuan/git/ai4ose-lab1/doomgeneric/doomgeneric/libc_shim.c#323-326)）和格式化字符串 `"STCFN%.3d"` 来生成 Lump 名称。`%.3d` （或 `%03d`）的作用是将整数格式化为至少 3 位，不足前面补零。

由于当前的 OS 环境没有任何现成的 C 标准库，我们在移植时手写了一个精简版的替换库—— [libc_shim.c](file:///home/donjuan/git/ai4ose-lab1/doomgeneric/doomgeneric/libc_shim.c)。既然格式化输出有问题，必定是手写的 [printf](file:///home/donjuan/git/ai4ose-lab1/doomgeneric/doomgeneric/libc_shim.c#339-342) 家族函数存在缺陷。

## 5. 审查 libc_shim.c：定位并解决根因

打开 [libc_shim.c](file:///home/donjuan/git/ai4ose-lab1/doomgeneric/doomgeneric/libc_shim.c) 并查看核心格式化函数 [_vformat](file:///home/donjuan/git/ai4ose-lab1/doomgeneric/doomgeneric/libc_shim.c#223-319) 的整数处理部分：

```c
/* flags/width/precision (simplified) */
// ... 解析了 pad 和 prec 等参数 ...

/* Emit tmp with padding */
int padding = pad > tlen ? pad - tlen : 0;
char pch = (zero && !left) ? '0' : ' ';
if (!left) for (int i = 0; i < padding; i++) PUTC(pch);
for (int i = 0; i < tlen; i++) PUTC(tmp[i]);
if (left) for (int i = 0; i < padding; i++) PUTC(' ');
```

> [!WARNING]
> **Bug 查明:** 虽然 [_vformat](file:///home/donjuan/git/ai4ose-lab1/doomgeneric/doomgeneric/libc_shim.c#223-319) 成功解析了格式化字符串中的精度（precision，如 `.3`），但在最终生成字符串时，完全**忽略了精度对于整数必须补充前导零的要求**。它仅仅处理了宽度（width，如 `%3d`），且没有正确设置 [zero](file:///home/donjuan/git/ai4ose-lab1/tg-ch8/customizable-buddy-fix/src/lib.rs#352-356) 标志位。
>
> 这导致 `"STCFN%.3d"` 在传入数字 33 时，被格式化为了 `"STCFN 33"` 或 `"STCFN33"`，失去了关键的前导零 `"0"`，导致引擎去查找错误的 Lump 名称，进而引发了一连串的崩溃惨剧。

**修复方案：**
重构这部分的 padding 生成逻辑，强制在配置了精度或 [zero](file:///home/donjuan/git/ai4ose-lab1/tg-ch8/customizable-buddy-fix/src/lib.rs#352-356) 标志时，补足前导零。

```diff
-        int padding = pad > tlen ? pad - tlen : 0;
+        int padding = 0;
+        if (have_prec) {
+            padding = prec > tlen ? prec - tlen : 0;
+            zero = 1; /* Precision on integers forces leading zeros */
+        } else {
+            padding = pad > tlen ? pad - tlen : 0;
+        }
```

## 6. 验证修复

修改 [libc_shim.c](file:///home/donjuan/git/ai4ose-lab1/doomgeneric/doomgeneric/libc_shim.c) 后，重新执行交叉编译和镜像打包流程，并再次启动 QEMU：

```bash
make -f Makefile.tgos -j8
cargo run --release -- -s ../tg-user/src/bin/ -t ../tg-user/target/riscv64gc-unknown-none-elf/release/
python3 run_doom2.py
```

此时查看运行日志：

```text
[DEBUG] Found font lump: STCFN036
[DEBUG] Found font lump: STCFN037
[DEBUG] Total STCFN font lumps found: 64
...
HU_Init: Setting up heads up display.
ST_Init: Init status bar.
...
```

**成功！** 找不到 `STCFN` 字体的报错完全消失，日志显示顺利找到了 `STCFN036` 等 Lump。程序安然度过了 [HU_Init](file:///home/donjuan/git/ai4ose-lab1/doomgeneric/doomgeneric/hu_stuff.c#286-309) 阶段，不再触发 `LoadPageFault`。

---

## 总结

这次 Debug 是一次非常经典的**从底层硬件异常追溯到顶层业务逻辑漏洞**的案例：

1.  不要盲目修改底层（如内核 [sys_write](file:///home/donjuan/git/ai4ose-lab1/doomgeneric/doomgeneric/libc_shim.c#47-48) 页表映射），仔细审视异常寄存器：`stval = 0x3` 这个微小的指针偏移量就是指路明灯，明确宣告了这是用户态的空指针解引用。
2.  日志分析与逆向工具（`objdump`）的结合：反汇编确认了操作的对象，日志则进一步说明了该对象为何为空（底层数据缺失）。
3.  警惕自制底层库：在移植环境中手写的 Shim 库（如 [libc_shim.c](file:///home/donjuan/git/ai4ose-lab1/doomgeneric/doomgeneric/libc_shim.c)）在处理边缘条件（比如特殊的 `%.3d` 格式控制符）时极易出错，这就是隐藏得最深的“原罪”。
