# extensions/

GNOUS 的扩展都放在这里，一个扩展一个目录，与本项目同仓开发。

- 约定与架构说明：[`../docs/EXTENSIONS.md`](../docs/EXTENSIONS.md)
- 新建扩展：复制 [`_template/`](./_template/) 为 `extensions/<extension-id>/`
- 大多数扩展只需一份 `manifest.toml`（声明式配置，界面自动生成表单）；
  只有"页面级"扩展才需要写 Rust 代码、在 `registry.rs` 中注册左栏页面。
