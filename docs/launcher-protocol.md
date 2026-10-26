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

披风库位于皮肤页的“已保存”区域，支持 PNG 上传、穿戴、脱下、从衣柜移除。上传先保存到衣柜，不自动替换当前穿戴；仅 YSUMC 账号显示写入入口。客户端限制为宽高比 2:1、最大 1024×512、1 MiB，服务端继续执行自己的大小、名称和角色所有权校验。原生 `minecraft-skins` 插件增加 `upload_cape(accountId, textureBlob, name)`、`equip_cape(accountId, capeId?)`、`remove_cape(accountId, capeId)`，均核对当前账号与页面账号一致；穿戴披风请求省略 skin 槽位，保留原皮肤。入口有尺寸提示，沿用既有基础引导，不增加首次启动步骤。

Windows 默认游戏资源目录为启动器 EXE 同级的 `data/`，其中包含 profiles、meta、caches 等；设置、账号数据库与日志仍在 AppData。旧默认目录在升级时迁移，保留玩家明确指定的自定义目录和开发隔离目录。迁移前保存源/目标信息以供失败后重试，拒绝与目标已有游戏数据合并。

卸载实例会同时删除对应游戏目录、本地 profile、同步生成文件与实例备份，并清除官方整合包的本地绑定。后台配置写入与卸载串行，避免删完后重新创建目录。下载的官方 mrpack 仅在所有现存 release/beta 数据库均无引用、无安装或恢复任务时清理；其他实例仍需离线校验的包、未知缓存和未完成下载均保留。独立的外部版本目录可删除，共享 Minecraft 根目录不随单个实例删除。首页监听实例事件，并以本地安装记录计算“安装/更新/启动”状态，防止较晚返回的云端清单恢复已删除的绑定。

列表缓存 20 秒，按账号、角色与 token 指纹区分；请求取消后可重新读取。上传、穿戴、删除使缓存失效；写操作期间返回的旧列表不会重新写入缓存。读取超时 20 秒、写入超时 30 秒，覆盖响应体读取。

## 8. 公告和更新

社区公告为公开读取，前端兼容数组及包装结果。它与启动器版本公告分别管理，不自动生成发布说明。

1.0.2 的版本公告统一读取 `GET /api/launcher/update/releases`，返回 `{releases:[{version,notes,pub_date,published_at}]}`。数据来自后台已启用的正式版发布记录，按版本倒序、相同版本按发布时间及记录 ID 去重，最多 100 条。公告不依赖安装包托管位置、摘要或签名，因此外部托管的安装包也能正常展示对应说明。版本历史与更新后弹窗共用该数据源，不再读取旧 Axolotl 内置历史。Markdown 使用现有清理器渲染，外链仅允许 HTTP/HTTPS。

打开更新设置、点击检查更新或打开更新后公告时重新请求；失败时保留本地最近成功的公告缓存并提示重试。成功响应整体替换缓存，包含后台撤下版本和空列表。更新检查还会缓存动态更新响应中的目标版本说明，以便安装后离线展示。启动弹窗顺序与 `pending_update_toast_for_version` 的关闭后清除规则保持不变。

未设置过更新偏好的用户默认立即获取正式版更新，保留用户明确关闭或暂停更新的选择。手动检查跳过可选的 24 小时等待；可安装/需确认下载的提示也取消额外 24 小时延时，仍按版本和阶段只提示一次。更新设置移除渠道切换、数据库隔离展示及复制按钮，更新请求固定使用 `release`；原有账户与实例数据库位置不变。此调整简化已有设置，不新增首次使用指引。

更新渠道通过查询参数 `channel` 或 `X-YSUMC-Channel` 传递，兼容旧 `X-Axolotl-Channel`。Tauri 动态更新包含版本、制品 URL、签名、说明和发布时间；没有适用更新返回 204。

发行构建使用 `apps/app/ysumc-updater.pub` 的 YSUMC 公钥，也可用 `YSUMC_UPDATER_PUBLIC_KEY` 覆盖。`tauri-release.conf.json` 已开启更新签名制品；私钥只保存在本机受限权限的 `.ysumc-signing/updater.key`，不上传网站、不入库；Tauri 本地签名命令已用临时文件验证成功。构建时通过 `TAURI_SIGNING_PRIVATE_KEY` 指向该本机文件，`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 留空。不要把空签名、未签名二进制或占位版本作为可安装更新发布。

`/api/launcher/update/versions` 已部署，返回 `{versions:[{version,artifacts:[{kind,platform,architecture,relative_path,sha256,size}]}]}`。后台发布表单支持 SHA-256、字节大小和类型；未启用、非正式渠道、元数据不完整或站外 URL 不进入该下载目录。该接口不作为版本公告来源；动态 Windows 更新仍可使用有效的站外安装包 URL 和签名。用户发布 1.0.1 后，实测当前版本 1.0.0 得到 200/1.0.1，而当前版本 1.0.1、1.0.2 得到 204。公网公告接口已返回后台填写的 1.0.1 与 1.0.0 原文；此验证不等于已完成原生覆盖升级。

## 8.1 CurseForge 下载源

下载页面可进入“下载源 · CurseForge / Modrinth”；CurseForge 提供自动、镜像优先、官方优先和仅官方。MCIM 路由依据 [其官方文档](https://docs.mcimirror.top/)，API 与文件 CDN 均有镜像映射；批量项目/文件查询和指纹查询允许只读 POST 镜像路径。

1.0.1 的官方 API 通过 `https://skin.ysumc.net/api/launcher/curseforge/v1/...` 访问。启动器仅向本站附加 YSUMC Bearer 会话，镜像不收到会话凭据。本站验证 Janus 会话后，使用服务器 `.env` 中的 `YSUMC_CURSEFORGE_API_KEY` 请求固定的 `https://api.curseforge.com`。Key 不进入前端、编译环境、安装包或公开响应；构建脚本不再嵌入 `CURSEFORGE_API_KEY`。代理限定 GET 元数据和只读批量 POST，限制请求大小与路径，关闭重定向，保留每分钟 60 次限流，并短暂缓存成功的公开元数据。

镜像的 HTTP 错误、连接异常、响应中断或无效 JSON 会继续尝试下一来源，不等待失败镜像的 `Retry-After`。全部来源失败时仍保留真实失败状态；分类失败在下载页内提示，不弹出原始 Theseus 错误。官方源也可独立选择，不再依赖客户端 API Key。

官方文件 CDN 仍由启动器直接下载并验证原有文件摘要，不经过皮肤站传输。备用 ForgeCDN 地址对文件路径中的 `+` 编码为 `%2B`，保留已有转义与查询参数；实测 Fabric API 的 edge 地址返回 404，而编码后的 mediafilez 备用地址正常返回 206。

皮肤站公开 PNG 纹理响应补充 `Access-Control-Allow-Origin: *`，使 WebView 可以将披风用于 WebGL 预览。启动器对旧纹理缓存添加预览版本参数；可选披风失败时仍渲染皮肤本体，且不把缺失披风的缩略图存入持久缓存。真实皮肤或模型失败仍显示失败提示。

2026-10-21 实测 MCIM 返回 HTTP 503。配置服务器 Key 后，本站代理使用真实 YSUMC 会话查询分类、搜索项目和批量项目均返回 200，未登录返回 401，其他游戏查询返回 400；公开披风通过浏览器跨域读取并成功绘制到 Canvas。公网访问 `.env` 返回 403。已验证官方 CDN 备用地址可读取文件内容，完整原生下载、安装及游戏启动仍需在桌面端验收。

## 8.2 mc-interop v1 接收（2026-10-22）

启动器合并读取本站 `/api/launcher/manifest`、`https://skin.ysumc.net/.well-known/mc-interop-v1.json` 和 `/api/launcher/friends`。规范主清单只含本站；客户端发送 `X-YSUMC-Catalog: split-v1`，友站从 friends 的 `sites[].instances` 获取，分组元数据映射回 `source`，保留 ID、SHA-512、stale 和纯服务器信息。friends 成功返回空数组表示撤回；任一清单请求失败则使用上次完整离线快照，避免误退役友站。实现位于 `packages/app-lib/src/api/managed/interop.rs`，参考皮肤站插件的同名协议文档与 SIT 启动器既有的受管实例流程。`managed_sync` / `managed_install` 的 IPC 名称及权限链保持兼容；传入的 manifest URL 仍是固定的本家配置标识。

- **目录与来源**：保留 `packs` 数组顺序，映射名称、简介、MC/加载器版本、包大小、服务器地址，以及 `source.siteId/domain/stale`。按同一文档的 `peers` 中匹配的 `siteId` 和 `domain` 补充 `source.name/description`；旧缓存缺少这些字段时显示域名。本站 `survival` 与 `sitmc:survival` 使用不同数据库键；支持每段最长 128 字符的命名空间，ID 不用作文件路径。首页只显示本站实例，友社页按来源社团分组，显示社团名称、简介、域名及过期来源提示。客户端不会向 `peers` 中的地址自行拉取目录，聚合范围仍由皮肤站管理。
- **发现与缓存**：请求验证 HTTPS 和固定本家站点身份，不发送账户凭据，不跟随目录重定向；15 秒超时，失败最多重试 3 次，流式响应上限 1 MiB。主清单与 friends 的原始文档和 ETag 分别按各自 URL 存在既有 `managed_manifest_cache` 中，条件请求支持 304。规范化快照继续使用原缓存键，保留旧版本安装记录；不改历史迁移。启动/主页首次加载、每 6 小时、窗口恢复焦点（距上次成功至少一分钟）、手动刷新及主页启动游戏前同步。目录不可达时标记离线并保留最近有效快照，阻止新安装/更新。
- **本站发布与公共上架**：`/api/launcher/manifest` 决定本启动器的官方 mrpack（含 SHA-1）条目、当前版本、服务器信息、图标、开服日期、升级策略和云地图。已启用且包信息完整的实例即下发，不要求在“对外清单”上架；公共开关只控制互操作目录。合并时本站 manifest 覆盖同 ID 的公共 mrpack，加入未公开的已发布条目，移除本站已撤回的兼容 mrpack；友社命名空间隔离，纯服务器和仅支持新协议的包仍由互操作目录提供。本站 manifest 请求失败时使用既有离线快照路径，禁止将旧版本当作已确认的新清单安装。升级公告仍走原发布接口。
- **安装与完整性**：mrpack 继续使用原安装任务、下载进度、重试与安装成功事务。支持 SHA-1 或 SHA-512，二者都有时以 SHA-512 为准；缓存文件以对应摘要命名，启动前的模组校验与跨渠道缓存保留也使用同一摘要。`multimc`、`curseforge-zip`、`server-zip`、`raw` 目前只展示并禁用安装，不假装安装成功；未知枚举和坏条目被跳过、记录诊断并在首页提示。
- **纯服务器**：省略 `artifact` 的条目按指定 MC 与加载器版本创建实例，由本地生成的无内容 mrpack 适配到原事务安装流程，不下载不存在的整合包。游戏/加载器要求变化时更新本地对账版本。它不执行整合包模组修复。省略端口保留为未指定，交给 SRV/客户端默认；显式 25565 也保留，IPv6 正确加括号。
- **用户数据**：撤回目录条目仅退役记录，不删除游戏目录或存档。源站更新不会改变本家登录身份；互操作目录不代表跨站账户互认，实际进服仍由目标服务器的认证配置决定。

公网只读验证：新清单返回 200、携带 ETag 的条件请求返回 304；验证时包含 1 个 YSU 与 4 个 SIT 条目。匿名下载 `sitmc:parkour` 得到 19,560 字节，SHA-1 与清单的 `b4a144477c6a995511a10c7ac5a9b00338b0b243` 一致，包内 MC 版本为 1.21.10。临时样包已删除。此检查不等于完整原生安装或真实账号进服验证。

本轮代码验证：受管实例/互操作 Rust 测试 15 项、缓存测试 7 项、前端 Node 测试 692 项、首页浏览器测试 7 项通过；前端类型检查、生产构建、作用域 ESLint/Prettier、Rust 格式检查及 `cargo check --package theseus_gui --features updater --locked --offline` 通过。首页测试覆盖来源/过期标记、纯服务器与不支持格式、安装时保留友站 ID，以及启动前发现新版时阻止使用旧实例。浏览器使用隔离的无界面 Edge 与模拟 IPC；原生命令 handler/build.rs/main 窗口 capability 已静态核对，本轮未启动真实桌面客户端做安装和进服测试。临时浏览器配置已移除，未改版本号、重新打包 EXE 或发布云端更新。

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
- 1.0.0 已使用 Release 优化及 fat LTO 构建，Windows x64 NSIS 安装包和更新签名交付至 `../releases/YSUMC-Launcher-1.0.0/`。安装包为 `YSUMC-Launcher_1.0.0_windows-x64-setup.exe`，72,434,398 字节，SHA-256 为 `1146763389fcb050a281086caa97927acc64bc42febb08f18bef070a76046ee7`；同目录保留 `.sig` 与 `SHA256SUMS.txt`。
- 交付验证通过：7-Zip 全包完整性、包内主程序 ProductVersion/FileVersion 均为 1.0.0、x64 架构、Ed25519 更新签名及可信注释签名。包内二进制与构建产物仅相差 Tauri 正常写入的 3 字节安装类型标记（`UNK` → `NSS`），其余字节完全一致。本轮未执行安装包覆盖安装、实际游戏进服或上传云端更新目录；原 1.10.0-beta.3 版本号高于 1.0.0，需手动运行此安装包切换。
- 数据库降级脚本全部通过（包括 Windows 运行中保护）。
- 1.0.1 修复验证：89 项 CurseForge Rust 测试、18 项相关 Node 测试、13 项浏览器渲染/分类测试、前端类型检查、格式检查及 PHP 43 项契约检查通过。服务器代理和纹理 CORS 修复已部署。
- 1.0.1 Release 优化及 fat LTO 构建成功，Windows x64 NSIS 安装包交付至 `../releases/YSUMC-Launcher-1.0.1/YSUMC-Launcher_1.0.1_windows-x64-setup.exe`，72,676,862 字节，SHA-256 为 `8c10180b1f4b598c293fe5877195674fcbf3914ea1aaeb8c79544d03352e83dc`。同目录提供 `.sig` 和 `SHA256SUMS.txt`。
- 1.0.1 安装包完整性、包内 ProductVersion/FileVersion、x64 架构、Ed25519 更新签名及可信注释签名全部通过。包内主程序仅有预期的 Tauri 安装类型标记差异。对安装包、解包文件、主程序和前端产物共 1,688 个文件扫描，未发现所配置 Key 的 UTF-8、UTF-16LE 或 Base64 表示；本地临时 Key 文件已删除，部署配置由服务器保管。本轮未执行覆盖安装、原生完整下载或真实游戏进服。
- 1.0.2 更新修复验证：4 项更新策略/数据校验 Node 测试、4 项浏览器组件/缓存/Markdown/提示测试、前端类型检查与生产构建、作用域格式检查、Rust 格式与启用 updater 的原生编译检查、PHP 48 项契约检查通过。浏览器测试使用本机 Edge 的 Chromium 内核及模拟原生 HTTP 调用；公网接口另行验证，未将组件测试视为桌面原生端到端验收。说明文件检查通过（62 项）。
- 1.0.2 Release 优化及 fat LTO 构建成功，交付目录 `../releases/YSUMC-Launcher-1.0.2/` 包含 EXE、`.sig` 和 `SHA256SUMS.txt`。安装包为 `YSUMC-Launcher_1.0.2_windows-x64-setup.exe`，72,542,580 字节，SHA-256 为 `97b7db798582173b07c485f89a1735bd2c37d57f3b5e1178aef22e5ea6ccb4c4`。7-Zip 完整性、包内 ProductVersion/FileVersion 1.0.2、x64 架构、Ed25519 更新签名及可信注释签名通过；包内主程序与编译结果仅相差预期的 Tauri 安装类型标记。本轮未覆盖安装现有启动器，也未新增云端 1.0.2 发布记录；公告接口已部署，安装包需上传并发布对应记录后才会提供更新。
- 1.0.3 修复验证：存储迁移 4 项、卸载及跨渠道缓存保留 4 项、披风图片校验 1 项、CurseForge 回退 8 项 Rust 测试通过；前端相关 Node 测试 15 项、披风组件浏览器测试 3 项、原生命令注册一致性 4 项通过。前端类型检查、作用域格式检查、启用 updater 的原生编译检查和前端生产构建通过。浏览器测试使用模拟原生接口，不等于完成真实账号披风写入或原生覆盖升级。
- CurseForge 空筛选参数修复已部署：Laravel 将空字符串转为 null 后，代理仅在允许的查询字段中接受并移除空值，仍拒绝数组、未知字段与其他游戏。PHP 契约 53 项通过；同一服务端请求管线中，空搜索从 400 修复为 200，关键词和版本筛选继续返回 200；公网匿名请求继续返回 401。
- 卡巴斯基告警仍待实机报告核对。已交付 1.0.2 安装包内主程序 SHA-256 为 `eadcb21992aa06374e86300bd0f3cbdd5f4c5c62c504f1dc55b69c1d1ffb9560`，150,736,384 字节，Authenticode 状态为 NotSigned；更新签名验证不等同于 Windows 发布者签名。1.0.3 安装器仅在更新目标目录不可写时请求 UAC，减少原先普通用户目录更新中的不必要提权。此调整不构成报毒根因或误报已解决的证明；未修改安全软件设置。
- 1.0.3 Release 优化及 fat LTO 构建成功，交付目录 `../releases/YSUMC-Launcher-1.0.3/` 包含 EXE、`.sig` 和 `SHA256SUMS.txt`。安装包为 `YSUMC-Launcher_1.0.3_windows-x64-setup.exe`，71,646,919 字节，SHA-256 为 `58a10df0ec706c05dc3c56697a9bb9525d391331514cbf6dca4eb0e7e68823f4`。7-Zip 完整性、包内 ProductVersion/FileVersion 1.0.3、x64 架构、Ed25519 更新签名与可信注释签名通过。包内主程序为 148,934,144 字节，SHA-256 为 `3ca959cc7d1d70dc671728ab37bda6e7ae8203c118363ce06fe78661f38576aa`，与编译产物仅相差预期的 Tauri NSIS 安装类型标记。Authenticode 仍为 NotSigned；未执行覆盖安装、卡巴斯基实机复现或新增云端 1.0.3 发布记录。
- 经用户确认样本保存、共享与分析条款，已将原始 1.0.2 主程序和安装包提交至 Kaspersky OpenTIP，提交前再次核对了上述 SHA-256。[主程序报告](https://opentip.kaspersky.com/EADCB21992AA06374E86300BD0F3CBDD5F4C5C62C504F1DC55B69C1D1FFB9560/results)为 Clean，动态检测 0 项、可疑行为 0 项；[安装包报告](https://opentip.kaspersky.com/97B7DB798582173B07C485F89A1735BD2C37D57F3B5E1178AEF22E5EA6CCB4C4/results)也为 Clean，动态检测 0 项、低风险可疑行为 1 项、中高风险均 0 项。公开系统行为详情注明为示例而非真实数据，不能据此推断该低风险行为或原 PDM 告警的根因。两份报告截图保存在 `../releases/YSUMC-Launcher-1.0.2/Kaspersky-main-exe.png` 与 `Kaspersky-installer.png`。本次结果仅对应已提交的 1.0.2 文件，不是 1.0.3 的检测结果，也不等于完成实机复现或厂商人工撤销误报。


## MUTMCL 品牌变体（界面预览阶段）

两种启动器保留同一份安装、下载、实例、皮肤、云地图及更新实现。`brands/catalog.json` 是品牌入口；前端用 `VITE_LAUNCHER_BRAND` 选择，原生壳根据 Tauri identifier 初始化 `branding::BrandPolicy`。不要用 Cargo feature 或在 app-lib 中读取品牌构建环境变量，避免每次切换品牌重编核心库。Turbo 的前端缓存键包含品牌和配置文件。

| 项目 | YSUMC | MUTMCL |
| --- | --- | --- |
| identifier | net.ysumc.launcher | live.mutmc.launcher |
| 深链 | ysumc | mutmcl |
| 登录 | YSUMC Yggdrasil | 拥有 Java 版的 Microsoft 账号 |
| 目录 | 本站 mc-interop、独立 friends 及原 manifest 补充配置 | `/api/launcher/mutmc/manifest` |
| 友社页 | 显示 | 不注册路由、不显示导航 |
| 更新、公告 | `/api/launcher/...` | `/api/launcher/mutmc/...` |

两种 identifier 分开账号、实例数据库和缓存。现有 `validate_ysumc_session` IPC 为兼容旧调用保留名称，其校验策略及官方整合包启动校验均由活动品牌决定；Microsoft 登录继续使用既有设备授权命令，不新增登录 IPC。MUTMCL 不接受离线或 Yggdrasil 账号通过登录门禁。更新下载地址与版本列表在原生层同样按品牌选取；更新公告缓存按品牌区分。

### 皮肤站管理

- 管理入口：`/admin/launcher/mutmc`，沿用 `web + authorize + role:admin`、CSRF 和既有上传/激活流程。新建实例通过该入口自动归属 MUTMCL，归属创建后不可修改。
- 分发：`GET /api/launcher/mutmc/manifest`；下载：`GET /api/launcher/mutmc/packs/{id}/{revision}`。只发布 MUTMC 的已启用、当前版本、SHA-1 完整的 mrpack，其他品牌、停用记录、未激活历史版本不可通过该下载路由取回。
- `distribution_brand` 向前新增到实例、公告和版本表，默认 ysumc，原有记录不迁移到 MUTMC。设置使用 `mutmc.` 前缀；MUTMC 不参与 YSUMC 的 mc-interop 聚合。
- 既有公告和版本管理表单新增所属启动器选择。MUTMC 的 announcements、update/latest、update/releases、update/versions 和动态更新路由独立筛选；没有记录时公告/版本历史为空、更新返回 204。
- 云地图地址、服务器地址、游戏版本、加载器和开服日期沿用现有字段。在线人数仍由原生 Minecraft ping 查询，不在后台填写虚拟人数。
- CurseForge 镜像/官方切换继续共用。MUTMC 官方备用请求走 `/api/launcher/mutmc/curseforge/{path}`，不向皮肤站发送 Microsoft token。该公共只读目录代理限定 Minecraft、路径/方法/参数/体积，30 次/分钟、缓存及上游超时；YSUMC 原有认证入口不变，API Key 只留在服务端。

### 预览与构建

运行 `node scripts/axolotl/brand.mjs mutmc preview`，打开 `http://127.0.0.1:5201/preview.html`。预览复用实际首页、品牌组件、主题和语言资源；Tauri mock 只存在单独的 dev 入口，不进入 index.html 或生产包。演示整合包不会写入后台，默认空清单。Microsoft 按钮在这个浏览器预览中只切换到演示首页，真实客户端则打开原有微软设备授权流程。

`node scripts/axolotl/brand.mjs mutmc config 1.0.5-beta.1` 输出 Tauri 覆盖配置，设置独立产品名、identifier、协议、原生图标、更新地址和用户指定的版本号，不重写现有 YSUMC 配置或 Cargo 版本。MUT 原生图标位于 `apps/app/icons/mutmc`，来源为网站原图。将生成的配置与现有 `tauri-release.conf.json` 按顺序合并，前端须使用 `VITE_LAUNCHER_BRAND=mutmc` 构建。Windows 预览安装包使用既有 `ci` Cargo profile 和 NSIS，保留更新签名；正式发布仍遵守 release 优化及发布检查，不将浏览器演示数据用于安装包。

原生启动时先从 Tauri 配置初始化品牌及包版本，再初始化日志和数据库；默认更新通道以实际安装包版本为准，因此 `1.0.5-beta.1` 使用 `live.mutmc.launcher/beta` 数据库。实例快捷方式使用 `mutmcl://` 和 MUT 图标，YSUMC 使用自己的协议和图标。启动本地验证程序时仍设置 `THESEUS_CONFIG_DIR` 隔离验证数据。后台代码需要单独部署，构建安装包不会发布后台或插入整合包记录。

构建耗时分析：上次 1.0.5 Rust release 用时 35 分 24 秒，其中主程序约 31 分钟，只有两个本地 crate 重编。根因是发布脚本强制 `CARGO_PROFILE_RELEASE_LTO=true`，覆盖 Cargo 默认的 `lto=false`。浏览器预览无需 Cargo；可在后续实际打包时采集 `--timings` 并评估 Thin LTO，但需要同步调整本地和 CI 发布门槛及验证体积/性能。本轮不改优化等级、签名要求或 Cargo target 目录。
