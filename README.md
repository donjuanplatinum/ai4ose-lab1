# AI4OSE
项目名称: AI4OSE
导师: 陈渝 王宏宁

本项目旨在响应清华大学 AI4OSE (AI for OS Education) 实验倡议，利用** AI 协作工具**，基于 **Rust 语言** 与 **RISC-V 64** 架构，构建一套**高度组件化**、**可观测**且**追求性能极致**的操作系统**教学实验环境**。

## 目标
- 原子性: 即环境的高度原子级的可复现 以助于学生不用花过多时间于环境配置(使用nix等工具)
- 工程级别: 借助于Github的workflow 所有的操作都经过完整的 严格的CI测试 并会发布于Crates.io
- AI协同: 借助大模型的优势 对知识体系进行建模与类比 并在每个章节最后由AI生成习题
- Bench集成: 使用Bench工具对内核性能进行评定 并由AI助手进行报告 以实现对缓存命中 系统调用时间等指标的分析

## 环境
- 文档: Sphinx Doc 作为文档记录 以整合Markdown知识
- AI: Google Gemini作为AI辅助助手
- OS: NixOS以实现原子级别复现
- Editor: Emacs


## 使用

### 文档
对于文档生成,您只需要部署好python环境 并使用pip安装依赖即可

```shell
pip install -r requirements-version.txt -i https://mirrors.tuna.tsinghua.edu.cn/pypi/web/simple
```

然后使用make工具进行文档生成
```shell
make html
```

最后您可以使用静态界面的服务器: Nginx等进行托管

