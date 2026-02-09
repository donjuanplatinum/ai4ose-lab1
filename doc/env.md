# 环境配置
一直以来 操作系统开发的环境配置相对复杂 涉及到交叉编译,qemu转译,平台限制 等诸多问题。 有时候甚至会因为distro的原因 或者qemu版本的原因导致各种bug。

为了解决配环境的问题 减少在环境上的时间 更快进入操作系统真正内容的学习 我们使用nix包管理器进行**原子级别**的环境配置.

对于Nix管理器 在这里您只用知道的是 `Nix 是一个基于函数式范式的声明式配置工具 你的环境具有原子性 函数的输入是怎样 环境就是固定的`

可以查阅[NixOS](https://nixos.org)来了解更多.

## 安装nix与配置环境
为了使用nix包管理器 我们需要安装nix(当然您也可以安装NixOS系统).

```shell
sh <(curl --proto '=https' --tlsv1.2 -L https://nixos.org/nix/install) --no-daemon
```

安装完nix后 执行这条命令
```shell
nix  --extra-experimental-features 'nix-command flakes' develop --option substituters "https://mirrors.tuna.tsinghua.edu.cn/nix-channels/store"
```

这条命令会根据仓库的flake.nix来构建环境
