# iOS 代理客户端获取 Garupa UID 与 UUID

这套脚本只用于自己的 BanG Dream! Garupa 账号调试。它只读取匹配请求的
`/api/user/<UID>` URL 和 `X-Signature` 请求头，不修改游戏请求，也不上传请求体、
加密 Key 或 IV。

## 支持的客户端

| 客户端 | 远程导入文件 | 远程脚本 | 额外说明 |
| --- | --- | --- | --- |
| Loon | [`penlight-credentials.plugin`](../ios/loon/penlight-credentials.plugin) | [`ios/common/penlight-credentials.js`](../ios/common/penlight-credentials.js) | 需 Loon 3.5.1 (983)+ / 3.5.2+；支持通知复制和手动显示 |
| Shadowrocket | [`penlight-credentials.module`](../ios/shadowrocket/penlight-credentials.module) | [`ios/common/penlight-credentials.js`](../ios/common/penlight-credentials.js) | 远程导入模块后自动捕获 |
| Surge | [`penlight-credentials.sgmodule`](../ios/surge/penlight-credentials.sgmodule) | [`ios/common/penlight-credentials.js`](../ios/common/penlight-credentials.js) | 远程导入模块，支持点击通知复制（iOS 5.11.0+ / Mac 5.7.0+）或手动运行 `penlight-credentials-show` |
| Stash | [`penlight-credentials.stoverride`](../ios/stash/penlight-credentials.stoverride) | [`ios/common/penlight-credentials.js`](../ios/common/penlight-credentials.js) | 远程导入 Override，主页 Tile 显示已保存内容 |
| Quantumult X | [`penlight-credentials.remote.snippet`](../ios/quantumultx/penlight-credentials.remote.snippet) | [`ios/common/penlight-credentials.js`](../ios/common/penlight-credentials.js) | 通过网页一键添加并启用远程 Rewrite 片段 |

所有客户端均从 [`ios/common/penlight-credentials.js`](../ios/common/penlight-credentials.js)
远程加载脚本，不再需要把 JS 文件保存到设备。

## 安装

请手动复制对应地址，粘贴到客户端的远程导入或添加页面。Loon 导入的是 Plugin，Stash 导入的是 Override，Quantumult X 添加的是远程 Rewrite 片段。

| 软件 | raw GitHub 链接 |
| --- | --- |
| Shadowrocket | https://raw.githubusercontent.com/Lanyinn-Labs/Penlight-Dream-Box/main/ios/shadowrocket/penlight-credentials.module |
| Surge | https://raw.githubusercontent.com/Lanyinn-Labs/Penlight-Dream-Box/main/ios/surge/penlight-credentials.sgmodule |
| Stash | https://raw.githubusercontent.com/Lanyinn-Labs/Penlight-Dream-Box/main/ios/stash/penlight-credentials.stoverride |
| Loon | https://raw.githubusercontent.com/Lanyinn-Labs/Penlight-Dream-Box/main/ios/loon/penlight-credentials.plugin |
| Quantumult X | https://raw.githubusercontent.com/Lanyinn-Labs/Penlight-Dream-Box/main/ios/quantumultx/penlight-credentials.remote.snippet |


## 使用步骤

1. 在网页首页“**安装抓取配置**”中点击对应客户端，直接打开远程导入页面。
2. 为 `api.garupa.jp` 开启 MITM，安装并在 iOS/macOS 系统设置中信任客户端 CA 证书。
3. 启用配置，打开游戏并进入个人资料或其他会访问 Garupa 用户接口的页面。
4. 通知正文包含完整 JSON；支持通知复制的客户端可点击复制，也可通过各客户端的手动查看入口读取已保存的 JSON：
   - **Shadowrocket**：若点击通知只打开应用、没有复制，请在脚本编辑/测试界面手动运行 `penlight-credentials.js`（不带请求，或参数设为 `show`），从本次运行日志复制完整 JSON。更新后请在当前配置执行“使用配置”或“编译配置”刷新远程脚本缓存。
   - **Surge**：模块仅向 `[MITM] hostname` 追加 `api.garupa.jp`，使用前请在 Surge 中手动开启 **MITM 总开关** 并信任证书。Surge iOS 5.11.0+ / Mac 5.7.0+ 支持点击通知复制；也可在「更多 -> 脚本（Scripting）」长按 `penlight-credentials-show` 运行（或在脚本编辑器/快捷指令触发）。更新后请在模块或外部资源中刷新远程脚本缓存。
   - **Stash**：请在 Stash 首页开启 **MitM** 并生成、安装且信任 Stash CA 证书，并确认未启用仅隧道代理（Tunnel Proxy Only，该模式会禁用 HTTP 引擎）。Stash 通知本身不支持点击自动复制，可展开通知横幅查看、刷新首页 Tile「显示已保存 UID / UUID」，或在脚本运行器中手动执行 `penlight-credentials-show` 从日志复制；更新后请刷新 Override / `script-providers` 缓存。
   - **Loon**：插件使用 **Loon 3.5.1 (Build 983)+**（App Store 3.5.2+）Script v2 语法，请确保客户端已更新到该版本及以上，并开启 **MitM** 与 **脚本（Script）** 总开关、安装并信任 Loon CA 证书。支持点击通知复制，也可在 VPN 运行状态下从插件/节点入口手动触发 `显示已保存 UID UUID` 查看结果或从日志复制；更新后如缓存未生效，可删除旧插件重新导入。
   - **Quantumult X**：网页一键导入通过 `quantumult-x:///add-resource` 直接打开应用并默认启用远程重写（`opt-parser=false, enabled=true`）。使用前请同时开启 **重写（Rewrite）** 与 **MitM** 总开关并安装信任 Quantumult X CA 证书，确认「重写 -> 资源」中 `Penlight Dream Box` 已启用（若当前配置未自动合并远程片段中的 `hostname`，请在 `[mitm]` 的 `hostname` 中手动加入 `api.garupa.jp`）。允许通知权限后点击通知可通过 `update-pasteboard` 复制；若受 iOS 剪贴板权限限制或未弹出通知，可展开通知查看完整 JSON，或在脚本编辑器中手动运行 `penlight-credentials.js` 从日志复制。

手动显示仅读取本机已保存的结果，不需要重新进入游戏。仅手动显示会把凭据输出到脚本日志，捕获请求时不会输出。

更新后，每次匹配到同时包含 UID 和 X-Signature 的请求都会再次通知，即使凭据
没有变化；游戏连续发送多条匹配请求时也可能收到多条通知。缺少请求头的请求
不会混用上次账号的凭据。通知横幅可能折叠内容，展开可查看完整 JSON。

复制出的结果形如：

```json
{
  "uid": "123456789",
  "uuid": "设备请求中的 X-Signature 值"
}
```

## 与服务端配置的关系

本项目把 `GARUPA_UUIDS` 定义为请求头 `X-Signature`，不是响应 protobuf 中另一个名为
`uuid` 的字段。旧的固定玩家采集流程可以把结果填入：

```dotenv
GARUPA_UIDS=123456789
GARUPA_UUIDS=设备请求中的 X-Signature 值
```

网页的动态 POST 导出接口不要求每次修改这两个值；只需在服务端预先配置解密 Key/IV。

不要把完整通知、代理客户端本地存储或 `.env` 上传到公共仓库；UID/UUID 应仅用于自己的账号。
