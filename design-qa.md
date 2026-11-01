# Oceanway Config 0.1.0 界面核对

- source visual truth: `apps/desktop/assets/release-notes/v4.0.0/main-zh.png`（固定 CC Switch v4.0.24 随附截图）。
- implementation: `.artifacts/oceanway-config-final.png`；本机浏览器 `fixture.html`。
- full-view evidence: `.artifacts/oceanway-design-final-comparison.png`。
- focused evidence: `.artifacts/oceanway-design-detail.png`，侧栏字体、图标、选中项与行内控件。
- viewport: 浏览器截图 1038 × 956；1x。源图 2144 × 1406，按 2x 归一为 1072 × 703 后同图比较。
- state: 简体中文浅色、连接成功、GPT/Claude/Gemini 分组、独立图片下拉。另核对深色 `.artifacts/oceanway-config-models.png`。

## 核对结果

这是原组件上的 Oceanway 接入流程，源图是供应商列表页；两者正文内容、窗口高度和操作系统窗口栏不同，不作逐像素还原结论。比较范围为沿用的侧栏、页头、字号层级、间距和控件风格。源图与实现并排及局部同图查看后，没有未处理的 P0/P1/P2 视觉问题。

| 检查面 | 结果 |
| --- | --- |
| 字体 | 沿用上游字体和 text-body/text-caption/text-section 层级；Windows 字体渲染与源 macOS 截图存在预期差异，无文字遮挡 |
| 间距与布局 | 200px 侧栏、原生页头、24px 正文横向留白、panel/control 圆角；内容可滚动，保存按钮完整可见 |
| 色彩 | 保留上游浅深色表面与语义颜色，Oceanway 操作颜色覆盖；焦点框清晰 |
| 图片与图标 | 使用 Oceanway 官方导出 symbol SVG，以及原应用图标；未重画品牌资产 |
| 文案 | 默认模型说明简短；完整真实模型目录可选；Key 密文，保存结果与加载配置分开 |

## 迭代记录

1. 早期预览发现窄侧栏使用横版标识后太小，改成品牌库 symbol SVG；之后局部图已确认尺寸与上游图标接近。
2. 用户指出每家只有一个示例模型。改为实际接口目录快照：7 GPT、13 Claude、5 Gemini、11 其它，另接用户图片测试服务的 11 个图片模型；实际 DOM 选项与快照逐项核对。生产读取实时 API。
3. 设置页残留 CC Switch 自称已更新为 Oceanway Config，保留关于页上游出处和 LICENSE。已移除当前版本无法执行的设置入口。
4. 最终截图在上述修复后重拍并再次与源图同图检查。

## 交互验证

- 填写 fixture-key、获取模型、各家默认选择、展开其它模型、提交后清空 Key。
- 一键配置不自动重启；结果提供单独加载按钮。
- 保存部分失败和图片失败由组件测试覆盖。
- 最终独立浏览器页控制台 error 为 0。
- 预览使用真实模型目录快照和模拟保存结果；原生安装和真实配置写入由单独集成测试验证。

final result: passed
