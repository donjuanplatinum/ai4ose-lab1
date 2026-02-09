# AI4OSE实验一
2026.02.09

## 一、引言

学生在当前操作系统内核的学习中，常见的现象是大家听同一个老师上课，看同一本教材，参加同样的应试考试。常见的问题是原理与实践脱节，缺少对知识点的全局理解，缺少历史发展的脉络，不知道操作系统内核是如何一步一步发展起来的，忽视硬件细节和软件架构设计。这些现存问题增加了学生学习和掌握操作系统内核的难度。另外一方面，现在AI辅助编程和AI问答的飞快演进，对操作系统内核教学也形成了机遇与挑战。
为此，我们希望对操作系统内核学习进行一定的调整，来帮助学生更有效地找到一个最适合自己的学习操作系统内核的方法手段。

我们的看法是：当一个学生学完操作系统内核这门课后，能给其他学生传授知识（1：理解知识），并开发操作系统内核（2：运用知识），那他就算是“懂”了操作系统内核这门课了。注：通过这门课的考试不代表他懂了操作系统内核。

## 二、目标
总体目标：与AI合作，高效完成对操作系统内核课的学习。
具体目标：与AI合作，参考最新的OS课组件化实验代码（持续更新）和附录中的可使用资源，设计一套适合自己学习的教学实验环境（包含实验指导文档、实验代码、测试用例/实验答案、文字类习题/文字类答案）。即做到自己只需基于自己写出的实验指导文档、实验代码、测试用例进行学习，就可以“懂”操作系统内核了。

## 三、要求：
1. 目前提供的“可使用资源”是超量的。文档、代码、习题等不在多，而在于精，特别是写出适合自己或与你类似的学生特点的教学实验环境为佳。所以这样的教学实验环境不是唯一的。
2. 操作系统内核编程语言是Rust，操作系统内核基于的硬件环境是RISC-V 64，操作系统内核实验在QEMU模拟器上运行。要求基于Rust Crate组件化编程，即每个内核或内核模块是可以发布到crates.io上的条件（cargo publish --dry-run 成功运行）
3. 文档格式是markdown格式，可以用mermaid on markdown来支持画图。
4. 自己设计的教学实验环境的git仓库（public）放在 https://github.com/learningos （我会帮大家建好）。写的不完善没关系，要求小步快跑，快速迭代，即一有更新，就push到仓库上，这样老师和其他同学都可以看到，并给出建议。
5. 不限制AI工具，充分与各种AI工具合作（需要把与AI工具交互的信息保存，并放到自己仓库中），但最终结果不是让自己降智了，而是高效地提高了自己的学习能力和技能
6. 每周写出周学习进展纪要（放在git仓库中），定期与老师交流讨论，改进学习过程，提高学习效率。

注：如没有特别的想法，可以基于附录中的最新的OS课组件化实验代码，进行改进、扩展、裁剪、重构，并添加相关文档，形成一套适合自己学习的操作系统内核教学实验环境。

## 附录：可使用的资源：
- 最新的OS课组件化实验代码（持续更新）：https://github.com/rcore-os/rCore-Tutorial-in-single-workspace/tree/test
  缺少匹配的实验指导文档。

课程：
- OS课程讲义列表： https://www.yuque.com/xyong-9fuoz/qczol5/glemuu?
- OS课程讲义源文件仓库：https://github.com/LearningOS/os-lectures

参考书：
- OS课教材参考书：https://pages.cs.wisc.edu/~remzi/OSTEP/Chinese/
- 深入了解计算机系统教材参考书：https://hansimov.gitbook.io/csapp/
- RISC-V Reader中文版：http://riscvbook.com/chinese/RISC-V-Reader-Chinese-v2p1.pdf

实验：
- rCore-Tutorial Book: https://rcore-os.cn/rCore-Tutorial-Book-v3/
- 实验代码：https://github.com/LearningOS/rCore-Tutorial-Code/
- 实验指导文档：https://learningos.github.io/rCore-Tutorial-Guide/
- 测试用例：https://github.com/LearningOS/rCore-Tutorial-Test/

注：这也是清华大学计算机系本科OS课学生学习所用的几乎所有资源（缺历年考试试卷、课后练习、上课视频）