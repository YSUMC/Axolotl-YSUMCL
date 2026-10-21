# YSUMC 启动器互通协议

本文对应 `packages/app-lib/src/ysumc.rs`、`api/managed.rs`、`state/minecraft_auth/oidc.rs` 和 `YSUMCUserCentre/plugins/sitmc-launcher` 的当前实现。生产服务地址为 `https://skin.ysumc.net`。整体开发、部署及验收清单见 [YSUMC 开发文档](../../docs/YSUMC系统复用开发文档.md)。

## 1. 边界

账号站提供游戏账号、角色、皮肤和启动器管理服务；官网 `https://ysumc.net` 为公开内容站。不开发展示范围之外的社员审核、报名绑定、学校 SSO、活动签到、官网登录或社员准入。

启动器进入主界面前必须先验证 YSUMC 登录。设备授权和账号密码登录均固定到本站；未登录时不挂载主路由。保留通用下载与普通实例；官方实例要求所选游戏账号来自本站。AI 与实验室导航关闭，新增云地图。其他账号类型的底层兼容代码保留，不作为绕过 YSUMC 登录门禁的入口。

外观设置提供 YSUMC 浅色、深色两种主题，切换立即生效并通过现有设置接口保存。强调色固定为协会 VI 青色，不提供其他主题或强调色选项；登录页与首页字标随主题切换。

## 2. 路由和认证

| 请求 | 负责组件 | 凭据 |
| --- | --- | --- |
| `GET /api/yggdrasil` | Yggdrasil Connect | 无，用于读取认证元信息 |
| `GET /api/janus/.well-known/openid-configuration` | Janus | 无 |
| discovery 中的设备授权、Token 端点 | Janus | 公开客户端设备流，不内置客户端密钥 |
| `GET /api/janus/userinfo` → `/yggc/userinfo` | Janus → PHP 插件 | Bearer access token |
| `GET /api/launcher/manifest` | 启动器插件 | 无 |
| `GET /api/launcher/packs/{instanceId}/{revision}` | 启动器插件 | 无 |
| `GET /api/launcher/announcements` | 启动器插件 | 无 |
| `GET /api/launcher/update/versions` | 启动器插件，带 SHA-256 和大小的制品目录 | 无 |
| `GET /api/launcher/update/latest` | 启动器插件 | 无 |
| `GET /api/launcher/update/{target}/{arch}/{currentVersion}` | 启动器插件 | 无 |
| `/api/launcher/skins`、`/skins/equip` | 启动器插件 | Bearer access token，服务端核验角色所属账号 |
| `/admin/launcher/*` | PHP 管理后台 | 站点管理员登录与 Web 中间件 |

Nginx 必须完整保留 `/api/janus` 前缀转发至 `127.0.0.1:13010（本次部署端口；开发默认 3000）`。Nest 挂载该前缀，再仅移除一次前缀交给 oidc-provider。代理应传递 Host 和 X-Forwarded-Proto；当前实现只信任本机代理，只监听回环地址。

## 3. 设备授权

1. 从固定 YSUMC Yggdrasil 根地址获取 discovery，校验返回的服务地址。
2. 从 discovery 读取 `shared_client_id`、设备授权、Token、userinfo 地址。缺少公开客户端 ID 应报配置错误，不能填入参考站的客户端 ID。
3. 请求范围：`openid offline_access Yggdrasil.PlayerProfiles.Select Yggdrasil.Server.Join`。
4. 前端显示授权代码，在同源浏览器页面选择游戏角色；按服务端间隔轮询，处理 pending、slow_down、过期、拒绝及取消。
5. 以 userinfo 返回的 `selectedProfile` 确认角色，保存独立的 YSUMC 提供方账号。刷新凭据仍使用此提供方，不调用 Microsoft 刷新接口。

浏览器授权是游戏账号授权流程，不建立官网登录会话。不要在日志中输出 access token、refresh token、device code 或私钥。

## 4. 官方整合包清单

`GET /api/launcher/manifest` 返回 schema 1：

```json
{
  "schema": 1,
  "generated_at": "2026-10-21T00:00:00Z",
  "cloud_map_url": null,
  "launcher": {
    "min_version": null,
    "latest_version": null,
    "force_update": false
  },
  "instances": [
    {
      "id": "survival",
      "name": "燕大生存服",
      "description": "官方生存整合包",
      "icon_url": null,
      "sort": 0,
      "revision": 1,
      "minecraft": {
        "game_version": "1.21.1",
        "loader": "fabric",
        "loader_version": "0.16.14"
      },
      "pack": {
        "kind": "mrpack",
        "url": "https://skin.ysumc.net/api/launcher/packs/survival/1",
        "sha1": "0000000000000000000000000000000000000000",
        "size": 1
      },
      "server": { "address": "mc.example.invalid", "port": 25565, "opened_on": null },
      "required": true
    }
  ]
}
```

这是字段示例，SHA-1、文件大小、服务器地址及版本必须换成实际制品的数据后才能发布。

约束：

- `id` 为 1–64 个 ASCII 字母、数字、下划线或短横线，不得重复。
- `revision` 为正整数；同一 `id` 不允许降低已知目标或已安装 revision。回退内容也应发布更大的 revision。
- `name`、`minecraft.game_version` 非空。包格式仅支持 `mrpack`，URL 必须为有效 HTTPS 地址且不含用户名、密码或 fragment。
- 包必须包含 40 位十六进制 SHA-1 和正文件大小；运行时下载沿用任务队列、进度、断点续传及完整文件校验。
- `launcher` 的非空版本值必须为可解析的 SemVer。
- `cloud_map_url` 为可空 HTTPS 地址，后台 `/admin/launcher` 配置；云地图页每 60 秒同步一次，远程页面不授予 Tauri IPC 权限。目标站必须允许 iframe 嵌入，不能嵌入时提供浏览器打开入口。
- `server.opened_on` 为可空 `YYYY-MM-DD` 开服日期，在对应实例后台编辑；按上海日期计算，开服当天为第 1 天。未填不显示天数。
- 在线人数不由皮肤站虚构或录入，启动器通过 Minecraft Server List Ping 查询配置的服务器，每 60 秒刷新；失败显示状态不可用。
- `required` 标识服务器推荐实例；**不触发静默下载，也不禁止玩家删除实例**。

## 5. 本地同步和断网行为

前端调用 `plugin:instance|managed_sync`，参数 `{manifestUrl}`；运行时只接受固定 YSUMC 清单地址。返回：

```ts
interface ManagedSyncReport {
  schema: number
  cloud_map_url?: string | null
  offline: boolean
  synced_at: number // 最后一次成功同步的 Unix 秒数
  launcher: ManagedLauncherPolicy | null
  instances: {
    spec: ManagedInstanceSpec
    instance_id: string | null
    applied_revision: number | null
    action: 'create' | 'update' | 'current' | 'retired'
  }[]
}
```

在线同步在验证完整清单后，以同一 SQLite 事务保存实例元数据和清单快照。解析、版本检查或写入失败不能留下半份新清单。

请求失败或服务返回无效清单时，重新验证上次成功保存的快照并返回 `offline: true`，保留原 `synced_at`。此过程不刷新缓存时间、不按旧快照退役实例，也不覆盖已知目标版本。没有可用快照则返回错误；首页仍保留“下载与发现”和“我的游戏”。

缓存模式显示上次同步时间，允许打开已安装的 current 实例，暂停从该清单发起安装或更新。重新连接并成功刷新后恢复按钮。打开实例不等于绕过启动检查，也不等于保证游戏认证在离线时可用。

安装和更新调用 `plugin:instance|managed_install`，参数 `{serverInstanceId}`。原生端重新同步固定地址清单并选取包信息，前端不能提交任意官方包 URL。安装任务持久化完整 `official` spec；任务成功、实例安装阶段、官方绑定、已安装 revision 和 `managed_installations.spec_json` 在同一数据库事务提交。重启恢复任务使用同一份 spec。重复创建任务会被拒绝，需在下载页恢复或取消原任务。

新迁移 `20261021060000_ysumc-official-installations.sql` 仅增加 spec 表，已加入数据库降级映射。旧版 `managed_register_instance`、`managed_mark_installed` 保留兼容接口，新首页不再依赖成功回调登记版本。

## 6. 启动、退役和普通实例

运行时启动入口检查官方绑定、实例安装阶段、已安装/目标 revision、退役状态及启动器最低版本策略。普通实例不受官方清单策略约束。

官方实例在每次启动前读取已安装版本的可信包，校验 `.mrpack` 的 SHA-1 和大小，从索引及 overrides/client-overrides 得到允许的 `mods/` 文件集。校验缺失/修改的模组并准备所有修复文件后，将额外及改动文件移至实例下 `ysumc-mod-quarantine/<随机ID>/`，再写入官方文件并复核。禁止符号链接、目录联接和越界路径。校验失败不启动；不扫描或重置 config、options.txt、存档。已经运行的官方实例拒绝重复启动。首次升级的旧绑定会从匹配已安装 revision 的可信清单恢复 spec，不能以用户当前 mods 目录作为官方基线。

官方实例的内容编辑和普通整合包替换入口由原生端拒绝，首页提供直接启动及右下角设置齿轮；内存、窗口等游戏设置可调整。

服务器下架实例只标记退役，不删除本地存档或文件。缓存中已退役的实例不会重新显示安装按钮。实际服务器地址和客户端/服务端 Yggdrasil 配置仍需通过进服联调验收。

## 7. 皮肤和披风

所有请求使用 `Authorization: Bearer <token>`。服务端通过受信任 userinfo 验证 token，再检查请求角色与材质权限，不能根据客户端提供的用户 ID 授权。

| 请求 | 输入 | 结果 |
| --- | --- | --- |
| `GET /api/launcher/skins?player=角色名` | 查询角色 | `{player:{name,skin,cape},textures:[...]}` |
| `POST /api/launcher/skins?player=角色名&type=steve&name=名称` | `Content-Type: image/png`，原始 PNG；type 可为 steve/alex/cape | `{texture:{tid,name,type,model,url}}` |
| `PUT /api/launcher/skins/equip` | JSON `{player,skin?,cape?}` | 按服务端验证结果更新穿戴 |
| `DELETE /api/launcher/skins` | JSON `{player,tid}` | 从本人衣柜移除材质 |

装备请求省略槽位表示不修改；值 `0` 表示卸下。编辑已穿戴皮肤时提交所选披风，选“无”即明确卸下，不再只更新 UI。默认/内置皮肤先作为 PNG 上传至本人衣柜，再穿戴。

写入运行时皮肤接受 64×32 或 64×64；历史高清皮肤预览按最近邻缩放到标准画布，不覆盖源图。衣柜按纹理数字 ID 从新到旧排列。服务端另有自己的材质限制。皮肤站 numeric tid 映射至启动器披风 UUID，非本站编码的披风 ID 会被拒绝。

列表缓存 20 秒，按账号、角色与 token 指纹区分；请求取消后可重新读取。上传、穿戴、删除使缓存失效；写操作期间返回的旧列表不会重新写入缓存。读取超时 20 秒、写入超时 30 秒，覆盖响应体读取。

## 8. 公告和更新

公告为公开读取，前端兼容数组及包装结果。后台内容与应用本地更新日志是不同来源，不自动生成发布说明。

更新渠道通过查询参数 `channel` 或 `X-YSUMC-Channel` 传递，兼容旧 `X-Axolotl-Channel`。Tauri 动态更新包含版本、制品 URL、签名、说明和发布时间；没有适用更新返回 204。

发行构建使用 `apps/app/ysumc-updater.pub` 的 YSUMC 公钥，也可用 `YSUMC_UPDATER_PUBLIC_KEY` 覆盖。`tauri-release.conf.json` 已开启更新签名制品；私钥只保存在本机受限权限的 `.ysumc-signing/updater.key`，不上传网站、不入库；Tauri 本地签名命令已用临时文件验证成功。构建时通过 `TAURI_SIGNING_PRIVATE_KEY` 指向该本机文件，`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 留空。不要把空签名、未签名二进制或占位版本作为可安装更新发布。

`/api/launcher/update/versions` 已部署，返回 `{versions:[{version,artifacts:[{kind,platform,architecture,relative_path,sha256,size}]}]}`。后台发布表单支持 SHA-256、字节大小和类型；未启用、非正式渠道、元数据不完整或站外 URL 不进入目录。没有发布制品时目录为空，动态更新返回 204。当前尚未产生本轮正式版安装包，不能据此宣称客户端自动升级验收完成。

## 8.1 CurseForge 下载源

下载页面可进入“下载源 · CurseForge / Modrinth”；CurseForge 提供自动、镜像优先、官方优先和仅官方。MCIM 路由依据 [其官方文档](https://docs.mcimirror.top/)，API 与文件 CDN 均有镜像映射，API Key 只发给官方源。无官方 Key 时镜像仍可显示搜索入口；批量项目/文件查询和指纹查询允许只读 POST 镜像路径。官方 Key 通过构建环境 `CURSEFORGE_API_KEY` 或本机运行环境 `AXOLOTL_CURSEFORGE_API_KEY` 配置，不写进提交或聊天。

2026-10-21 实测 MCIM 返回 HTTP 503；尚无本站 CurseForge API Key，因此两个来源都尚未完成真实下载验收。不要将源选择入口存在等同于第三方服务可用。

## 9. 验证边界

组件测试可验证缓存、安装按钮、退役提示和下载入口；Rust 测试可验证清单和本地事务；Janus HTTP 测试使用替代 OIDC 服务验证路由、CORS、重定向与配置，并不签发真实 token。

上线验收还必须使用同一套 PHP、Janus、MySQL、Yggdrasil Connect、OAuth 密钥及游戏服务器完成：注册、角色选择、设备登录、刷新、整合包安装和升级、皮肤/披风写入、认证进服、签名更新、备份恢复。占位域名与本地测试不构成生产联调证据。

### 2026-10-21 本轮验收记录

- 前端类型检查、683 项 Node 测试通过；首页/欢迎页 6 项 Chromium 测试、会话门禁 3 项测试通过；前端生产构建通过。
- PHP 契约 29 项、事务回滚验证 7 项通过；生产设备授权全 scope、角色选择、衣柜读取、皮肤上传/穿戴/移除、join/hasJoined 签名、令牌撤销、拒绝已撤销访问令牌与刷新令牌 11 项通过。
- 测试角色、测试私有纹理已清理；用户 643、角色 689、纹理 702 与测试前相同。认证插件的日志已去掉访问令牌正文。
- Minecraft 状态协议实测 `play.southmount.cn:25565` 返回 1/100（仅为测试时刻快照）。尚未启动真实游戏客户端进入该服。
- 新原生 IPC 已静态核对 handler/build.rs/capability 链；2026-10-21 最新桌面 Debug 构建已通过并在隔离测试目录启动，窗口正常响应。模组路径安全、覆盖优先级、修复及配置保留 3 项测试通过；官方绑定事务测试通过（Windows 测试进程设置 `RUST_MIN_STACK=16777216`）。完整原生交互和真实游戏进服仍需验收。
- 浅色/深色切换调整后，前端生产构建、主题与设置搜索 14 项 Node 检查、首页与欢迎页 6 项 Chromium 检查通过；用户已指定正式 EXE 版本为 1.0.0，版本元数据已统一。
- 数据库降级脚本全部通过（包括 Windows 运行中保护）。本轮正式版本号等待用户指定，尚未修改 beta 版本号或发布本轮安装包。
