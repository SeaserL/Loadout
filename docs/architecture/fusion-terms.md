# Loadout 资源与调用术语

沿用 [Loadout Glossary](glossary.md) 对 Provider、Account、Model、Pool、子号池与 RouteTarget 的既有定义。以下补充用于融合方案讨论，不重复定义既有实体。

## Language

**免费资源目录条目 Free Catalog Entry**：
关于某平台某模型免费资格、额度与有效期的资料记录，不表示用户当前一定能调用。
_Avoid_: 可用账号、已获得额度。

**规则核验时间 Terms Verified At**：
某条免费资格或额度规则最后被证据核验的时间。
_Avoid_: 最后同步时间、最近成功时间。

**目录同步时间 Catalog Fetched At**：
本机最近取得目录数据的时间，与该资料本身的新旧程度不同。
_Avoid_: 核验时间。

**账号可用性观测 Account Availability Observation**：
特定账号、模型与访问环境下的一次成功或失败记录。
_Avoid_: 平台永久可用、全局可用性保证。

**首选账号 Preferred Account**：
请求首先尝试的账号；在允许failover时，其失败可交由同模型的其他获准账号接手。
_Avoid_: 固定账号。

**固定目标 Pinned Target**：
本次请求或有状态会话唯一允许的推理目标；不可用时明确失败。
_Avoid_: 首选账号、普通会话粘性。

**实际路由轨迹 Routing Trace**：
请求执行时记录的候选选择、尝试、切换与结果事实。
_Avoid_: 当前策略重算、推测路径。

**用量流向 Usage Flow**：
在指定时间窗和计量口径下，调用来源与实际模型、账号之间的用量分布。
_Avoid_: token守恒、额度流入。

