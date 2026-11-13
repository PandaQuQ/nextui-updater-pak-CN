![](./doc/screenshot.png)

# NextUI OTA Updater PAK

用于在设备端（OTA）更新 NextUI 的 PAK。已在 TrimUI Brick 上测试（Smart Pro 也可能可用）。需要 Wi‑Fi（显然）。

## 安装

将 `nextui-updater-pak.zip` 解压到 SD 卡根目录（合并/覆盖同名文件即可）。

## 操作说明

- **方向键 上/下**：在按钮之间移动
- **A 键**：确认/选择
- **B 键**：返回/退出
- **X 键**：版本选择

## 版本模式

主界面最下方的「版本模式」按钮可以在两种模式之间切换（默认稳定版）：

- **稳定版**：只检查/列出正式发布的版本，跳过 GitHub 上标记为预发布（prerelease）的版本。
- **预发布版**：同时列出预发布（测试）版本，版本号后会标注「预发布」，并默认选中最新发布的版本。

设置保存在 SD 卡 `.userdata/shared/nextui-updater.txt`（`version_mode=stable` 或 `version_mode=prerelease`），重启后仍然生效。

## 在电脑上测试（开发用）

桌面构建（`cargo build`）可直接在 Linux 下运行，键盘 上/下/回车/X/Esc/左/右 对应手柄按键。两个环境变量便于测试：

- `NEXTUI_SDCARD_ROOT`：模拟的 SD 卡根目录（默认 `/mnt/SDCARD/`）
- `NEXTUI_UPDATER_API_BASE`：GitHub API 地址（默认 `https://api.github.com`），可指向本地模拟服务器

注意：更新完成后会执行 `reboot`，测试时请在 `PATH` 里放一个假的 `reboot` 脚本。

## 使用 [cross-rs](https://github.com/cross-rs/cross) 为 tg5040 构建

```bash
cross build --release --target=aarch64-unknown-linux-gnu
```

编译产物在 `target/aarch64-unknown-linux-gnu/release/nextui-updater-rs`。

## 打包发布 zip

```bash
scripts/create_pak.sh
```

生成的 zip 在 `./nextui-updater-pak.zip`。

## 许可证

本项目开源，使用 MIT License。

## 贡献

欢迎贡献！如果你有任何改进想法，欢迎提交 Pull Request。
