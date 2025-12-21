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
