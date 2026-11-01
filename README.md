# Picline for Windows

Mac 上那条「截图晾衣绳」的 Windows 版。仓库现在只有第一步：**验证最顶上那条边能不能用来唤出绳子**，其它功能一律还没做。

## 这一版做了什么

| 能用 | 说明 |
| --- | --- |
| 贴顶唤出 | 指针停在任意屏幕最上面 3 像素内满 0.25 秒，一条空绳从屏幕顶边滑下来 |
| 自动收回 | 指针离开绳子区域 0.5 秒，绳子滑回去，窗口隐藏，不留任何痕迹 |
| 快捷键 | `Ctrl + Alt + T` 放下 / 收起；`Ctrl + Alt + Shift + Q` 退出 |
| 托盘菜单 | 右键托盘图标，同样两项 |
| 多屏与缩放 | 在哪块屏幕上贴顶，就下在哪块；按该屏的缩放比例绘制 |
| 全屏不打扰 | 检测到全屏应用或演示模式时不出来 |
| 单实例 | 重复双击不会出现两条绳子 |

| 还做不到 | 说明 |
| --- | --- |
| 挂图 | 绳子上永远是空的，只画了一条绳子加一句提示 |
| 收图 | 剪贴板 / 截图文件夹监听还没接 |
| 交互 | 窗口是点击穿透的，图钉、拖拽、右键菜单都没有 |
| 多桌面 | 虚拟桌面切换时窗口不会自己跟过去 |

## 怎么试

1. 把 `Picline-0.1.0-win-x64.exe` 拷到 Windows 机器上（ARM 机器用 `-win-arm64.exe`）。
2. 双击。首次运行会因为未签名被 SmartScreen 拦一下：点「更多信息 → 仍要运行」。
3. 把鼠标推到屏幕最上面，稍微停一下；绳子应该滑下来。
4. 把鼠标往下移开，绳子应该自己收回去。
5. 右键托盘图标可以退出。

重点观察三件事，这就是这一版存在的意义：

- 绳子下来的时候，**会不会误伤最大化窗口右上角的关闭 / 最大化按钮**（那三个按钮也在最上面几像素里）。
- 贴顶 0.25 秒这个手感**是太灵敏还是太迟钝**。
- 绳子和提示文字在**浅色 / 深色桌面**上是否都看得清。

## 在 Mac 上编译

不需要 Windows，也不需要虚拟机。工具链：Rust + llvm-mingw 交叉链接。

```bash
# 1. Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
export PATH="$HOME/.cargo/bin:$PATH"
rustup target add x86_64-pc-windows-gnullvm aarch64-pc-windows-gnullvm

# 2. llvm-mingw（提供 linker 和 Windows 导入库）
#    https://github.com/mstorsjo/llvm-mingw/releases 下的 *-ucrt-macos-universal.tar.xz
mkdir -p ~/toolchains && tar -xf llvm-mingw-*-ucrt-macos-universal.tar.xz -C ~/toolchains

# 3. 出包
scripts/build-win.sh both      # 产物在 dist/
```

`.cargo/config.toml` 里写的是这台机器上的绝对路径，换机器要改；包装脚本 `~/toolchains/bin/*-gnullvm-cc` 只是把 clang 的 target 钉死。

几个踩过的坑，记在这里省得再踩：

- Rust **没有** `aarch64-pc-windows-gnu`，ARM64 只有 `gnullvm` 和 `msvc`；x64 两个都有。
- `windows-gnu` 目标要 `-lgcc` / `-lgcc_eh`，llvm-mingw（LLVM 运行时）里没有，所以走 `gnullvm`。
- `gnullvm` 默认动态链 `libunwind.dll`，那个 DLL 用户机器上没有。解法是把 llvm-mingw 的静态 `libunwind.a` 单独放一个目录，用 `-L` 让它被优先找到（`.cargo/config.toml` 里的 `static-unwind` 路径）。
- 产物只有系统 DLL 依赖（`gdiplus` / `user32` / `gdi32` / `shell32` / UCRT），Win10、Win11 都自带。

## 产物

| 文件 | 架构 | 大小 | PE |
| --- | --- | --- | --- |
| `dist/Picline-0.1.0-win-x64.exe` | x86_64 | 278 KB | PE32+ / Windows GUI |
| `dist/Picline-0.1.0-win-arm64.exe` | ARM64 | 220 KB | PE32+ / Windows GUI |

## 代码结构

| 文件 | 职责 |
| --- | --- |
| `src/main.rs` | 窗口、消息循环、贴顶状态机、动画、托盘、热键 |
| `src/canvas.rs` | 32 位 DIB + `UpdateLayeredWindow`，逐像素透明 |
| `src/gdiplus.rs` | 手写 GDI+ 绑定：抗锯齿曲线、圆角胶囊、一行文字 |

GDI+ 的绑定是手写的（没用 `windows` crate 的 GdiPlus 模块），这样升级 crate 时不会因为绑定增删而编译失败。
