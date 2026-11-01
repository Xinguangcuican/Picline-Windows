# Picline for Windows

Mac 上那条「截图晾衣绳」的 Windows 版。

## 这一版做了什么

| 能用 | 说明 |
| --- | --- |
| 贴顶唤出 | 指针停到任意屏幕最上面 3 像素内满 0.09 秒，绳子从屏幕顶边滑下来 |
| 自动收回 | 指针离开绳子区域 0.5 秒，绳子滑回去，窗口隐藏，不留痕迹 |
| 收图 | 剪贴板一变就去看：截图工具的 DIB、浏览器／工具的 PNG、资源管理器复制图片文件，全都收 |
| 挂卡片 | 每张图挂成一张卡片，新图在右边，最多留 12 张，最旧的从左边掉下去 |
| 即时反馈 | 收到图时绳子自己下来给你看一眼，1.6 秒后收回去（指针还停在上面就不收） |
| 全部取下 | 托盘右键 →「全部取下」，清空整条绳子 |
| 快捷键 | `Ctrl + Alt + T` 放下 / 收起；`Ctrl + Alt + Shift + Q` 退出 |
| 托盘菜单 | 右键托盘图标：放下 / 收起、全部取下、退出 |
| 多屏与缩放 | 在哪块屏幕上贴顶，就下在哪块；按该屏的缩放比例绘制 |
| 全屏不打扰 | 检测到全屏应用或演示模式时不出来，收到图也不弹 |
| 单实例 | 重复双击不会出现两条绳子 |

| 还做不到 | 说明 |
| --- | --- |
| 交互 | 窗口是点击穿透的，卡片点不了、拖不动，也没有右键菜单；要取图请从原始来源再取 |
| 多桌面 | 虚拟桌面切换时窗口不会自己跟过去 |
| 开机自启 | 要自己把 exe 的快捷方式放进启动目录 |

## 怎么试

1. 把 `Picline-0.1.1-win-x64.exe` 拷到 Windows 机器上（ARM 机器用 `-win-arm64.exe`）。
2. 双击。首次运行会因为未签名被 SmartScreen 拦一下：点「更多信息 → 仍要运行」。
3. 用微信截图、`Win + Shift + S`、或随手复制一张图片。
4. 绳子应该自己下来，图片以卡片形式挂上去，1.6 秒后收回。
5. 把鼠标推到屏幕最上面，稍微停一下，绳子带着那几张卡回来。
6. 右键托盘图标 →「全部取下」清空。

## 收不到图时

程序会把每次剪贴板变化记到 `%LOCALAPPDATA%\Picline\picline.log`，里面写着剪贴板当时到底有哪些格式，例如：

```
clipboard holds: CF_UNICODETEXT, CF_DIB, CF_DIBV5
took 1 picture(s) 1920x1080
```

如果某次截图没挂上，把那几行发出来就能看出是格式没认出来、还是读取时剪贴板被别人占着。

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
- 产物只有系统 DLL 依赖（`gdiplus` / `user32` / `gdi32` / `shell32` / `shlwapi` / UCRT），Win10、Win11 都自带。

## 产物

| 文件 | 架构 | 大小 | PE |
| --- | --- | --- | --- |
| `dist/Picline-0.1.1-win-x64.exe` | x86_64 | 310 KB | PE32+ / Windows GUI |
| `dist/Picline-0.1.1-win-arm64.exe` | ARM64 | 248 KB | PE32+ / Windows GUI |

## 代码结构

| 文件 | 职责 |
| --- | --- |
| `src/main.rs` | 窗口、消息循环、贴顶状态机、动画、卡片排版、托盘、热键 |
| `src/clipboard.rs` | 剪贴板监听与四种格式解析（HDROP / DIBV5 / DIB / BITMAP / PNG），收图去重，日志 |
| `src/canvas.rs` | 32 位 DIB + `UpdateLayeredWindow`，逐像素透明 |
| `src/gdiplus.rs` | 手写 GDI+ 绑定：曲线、圆角、文字、图片解码与缩放 |

GDI+ 的绑定是手写的（没用 `windows` crate 的 GdiPlus 模块），这样升级 crate 时不会因为绑定增删而编译失败。

## 已验证到什么程度

这台机器是 Mac，**exe 只能编译、不能运行**。已经做过的是：

- 两个架构都能交叉编译、链接，导入表里只有系统 DLL。
- `unpack_dib` 那段字节运算单独抽出来在本机跑过 7 组人造数据：32 位自下而上、alpha 全 0 视为不透明、24 位行填充、高度为负自上而下、`BI_BITFIELDS` 掩码跳过、短数据被拒、16 位被拒。

没做的是：真机运行。所以第一次跑还是要靠那台 Windows。
