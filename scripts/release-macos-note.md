---

> ⚠️ **macOS 首次运行**：安装包未做 Apple 公证，浏览器下载的应用带系统隔离标记，
> 首次打开会提示「已损坏，无法打开」——这是 Gatekeeper 对无证书应用的固定措辞，
> 文件本身没有损坏。任选其一：
> 1. 终端执行 `xattr -dr com.apple.quarantine /Applications/Hongguo.app` 后打开；
> 2. 下载本页 `install-macos.sh`，终端执行 `bash install-macos.sh`（自动校验
>    SHA256 并装入 /Applications，终端下载渠道不带隔离标记）。
>
> 经应用内「检查更新」升级不受此影响。
