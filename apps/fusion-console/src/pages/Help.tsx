// SPDX-License-Identifier: Apache-2.0
//
// The guide. Pages stay free of explanations because they are all here, and each page's "?" opens
// its own chapter. Everything said here was checked against what this build actually does.

import { type ReactNode, useEffect, useRef, useState } from "react";
import { ArrowRight } from "lucide-react";
import { CopyField } from "../components";

function Go(props: { to: string; children: ReactNode }) {
  return (
    <a className="guide-go" href={`#/${props.to}`}>
      {props.children} <ArrowRight size={13} aria-hidden />
    </a>
  );
}

function Terms(props: { items: [ReactNode, ReactNode][] }) {
  return (
    <dl className="guide-terms">
      {props.items.map(([term, text], i) => (
        <div key={i}>
          <dt>{term}</dt>
          <dd>{text}</dd>
        </div>
      ))}
    </dl>
  );
}

const CHAPTERS: { id: string; title: string; group?: string; body: (gateway: string) => ReactNode }[] = [
  {
    id: "start",
    title: "三步用起来",
    body: (gateway) => (
      <>
        <ol className="guide-steps">
          <li>
            <strong>添加账号。</strong>填 API 密钥，或者接入本机已登录的 WorkBuddy。添加后会自动同步这个账号能用的模型。
            <Go to="accounts">账号</Go>
          </li>
          <li>
            <strong>新建客户端密钥。</strong>每个使用 API 的软件各建一个，用量才分得开。密钥只显示一次，当场复制。
            <Go to="clients">客户端</Go>
          </li>
          <li>
            <strong>把地址和密钥填进你的软件。</strong>所有软件都填这一个地址。软件让你选协议时，选它支持的就行，OpenAI 或 Anthropic 都能用。
          </li>
        </ol>
        <div className="guide-addresses">
          <span>API 地址</span>
          <CopyField value={`http://${gateway}/v1`} />
        </div>
        <p>
          模型名有三种写法，从省心到精确：
        </p>
        <Terms
          items={[
            [<code key="a">loadout/分组名</code>, "你在模型页建的分组。账号挂了会自动换下一个，日常用这个。"],
            [<code key="b">服务商/模型</code>, "这家服务商的任一账号，按账号页的顺序选。"],
            [<code key="c">服务商/账号/模型</code>, "优先用这个账号。它不可用或不在密钥范围内时，由同一模型的其他账号接手。"],
          ]}
        />
        <p>
          关掉窗口，网关还在后台运行，软件照常能用。要完全退出，在系统托盘的 Loadout 图标上右键选“退出并停止网关”，或者到设置页点“停止网关”。
        </p>
      </>
    ),
  },
  {
    id: "dashboard",
    title: "总览：读懂流向图",
    body: () => (
      <>
        <p>
          左边是上游账号，右边是下游客户端，中间是网关。线束的粗细来自请求记录：选中的时间段里，谁走的量大，谁的线就多。
        </p>
        <Terms
          items={[
            ["点击一端", "聚焦这条流向：其他线变淡，对面只亮真正来自它的那部分，数字也换成这条流向的量，下方显示请求数和主要模型。再点一次、点空白处或按 Esc 退出。"],
            ["拖动一端", "调整这一列的上下顺序，只影响这个窗口的显示，和故障切换顺序无关。标题旁的小箭头恢复默认顺序。"],
          ]}
        />
        <p>动起来的部分全部来自真实请求，网关空闲时图是静止的。</p>
        <Terms
          items={[
            ["一个灰点走向上游", "请求刚发出去。之后线路微微流动，表示在等第一个字节。"],
            ["线束流动、光点从左到右", "上游正在回复。流动速度和光点密度跟着实际传给客户端的字节速度走，模型越快越密。中间的图标会随流量发亮。"],
            ["一小串光点", "这个请求太短，两次刷新之间就结束了，事后补一串。"],
            ["红色光点停在中间", "这次上游失败，没有到达客户端。如果分组里有下一个账号，接着会看到新的线路亮起。"],
          ]}
        />
        <p>
          图的两端还空着时，上面的“添加账号”和“新建客户端”就是下一步。关掉“界面动效”或系统动画效果时，只亮节点，没有光点。
        </p>
      </>
    ),
  },
  {
    id: "accounts",
    title: "账号",
    body: () => (
      <>
        <Terms
          items={[
            ["拖动账号卡片", "顺序就是故障切换的顺序，排在前面的先用。"],
            ["拖动服务商标题", "只调整页面上的排列。"],
            ["听诊器", "检查密钥是否有效，顺带清除认证错误。“全部检查”还会测一遍这家服务商每个启用的模型。"],
            ["闪电", "给这个账号的模型测速。"],
            ["刷新箭头", "重新拉取模型列表。模型显示 0/0 时点这里。"],
            ["收据", "填订阅价格和续费日，卡片上就能算出这个月的使用价值。"],
            ["刷新用量", "读取官方额度。只有点了才会访问上游。不支持读取的服务商，只显示 Loadout 自己记录的请求数和估算费用。"],
          ]}
        />
        <h3>子号池</h3>
        <p>
          页面顶部的下拉框。子号池是一部分账号的集合，切换过去后，所有路由只在这些账号里选，范围外的账号即使被点名也会被拒绝（HTTP 400）。适合临时隔离一个有风险的账号，或者分开工作和个人。切回“主号池（全部账号）”就恢复全部账号。
        </p>
        <h3>总 API</h3>
        <p>
          服务商标题上的“总 API”按钮。开启后这家服务商有一个专用地址 <code>/e/名称/v1</code>，客户端照常写模型名，请求只会发给这家的账号。勾选“每次请求轮换账号”后每个请求换下一个账号，额度平摊，但同一会话的提示词缓存可能失效。
        </p>
        <h3>导入和导出</h3>
        <p>
          “导出 API 账号”把所有账号打包成一个加密的 <code>.loadout-keys</code> 文件，口令至少 8 位，忘了就无法恢复。“导入 API 账号”把它还原到这台或另一台电脑，缺的服务商和账号会自动创建。
        </p>
        <h3>扫描本机</h3>
        <p>查找已安装并登录的 Codex、Claude Code 等工具。这些登录默认只读，用来显示额度，不用来转发请求。</p>
      </>
    ),
  },
  {
    id: "models",
    title: "模型与分组",
    body: () => (
      <>
        <p>
          右边是所有账号提供的模型，左边是你的分组。把右边的模型拖到左边的分组上就加入了，在分组里拖动调整顺序。一个分组对客户端来说就是一个模型名 <code>loadout/分组名</code>，点分组名可以复制。
        </p>
        <h3>分组怎么选账号</h3>
        <Terms
          items={[
            ["按顺序", "总从第一个开始，失败才往下。最容易预测。"],
            ["最快优先", "按测速结果从快到慢排。"],
            ["轮转", "每个请求从下一个开始，把量摊开。"],
            ["快重置优先", "先用快要刷新的额度，避免浪费。"],
            ["便宜优先", "按估算单价从低到高。"],
            ["最少使用优先", "让用量尽量均衡。"],
            ["随机", "每次随机选一个。"],
          ]}
        />
        <p>
          模型行上的开关决定它是否参与路由。档位下拉可以把模型固定在某个速度档，否则由测速决定。价格一栏（没有价格时显示“设价格”）可以点开手动填写，填了就优先用你的价格。
        </p>
      </>
    ),
  },
  {
    id: "prices",
    title: "比价和价格表",
    body: () => (
      <>
        <p>
          <strong>比价</strong>把同一个模型在不同账号上的价格放在一起，按你自己的输入、输出和缓存比例算出综合单价，最便宜的会标出来。只有一个账号提供的模型默认不显示。
        </p>
        <p>
          <strong>价格表</strong>是所有已知模型的参考价，单位是美元每百万 Token。点“立即同步”从 LiteLLM 的公开价格表更新，不同步也能用内置价格。
        </p>
        <p>
          估算费用时的优先顺序：你手动填的价格，然后是价格表里这家服务商的价格，最后是同名模型的价格。查不到价格时不当作免费，而是显示未知。这些都是估算，实际扣费以服务商账单为准。
        </p>
      </>
    ),
  },
  {
    id: "free",
    title: "免费模型",
    body: () => (
      <>
        <p>
          一份持续更新的免费模型清单，告诉你哪家服务商有什么免费额度。它只是目录，不会替你连接任何服务。列在这里也不代表你的账号一定能用，部分额度要实名或者限地区。
        </p>
        <Terms
          items={[
            ["已接入", "你已经有这家服务商的账号，排在最前面。去模型页同步一下就能看到这些模型。"],
            ["添加账号", "还没有账号。去这家官网注册拿到密钥，再回账号页添加。"],
            ["每月 / 一次性", "每月刷新的额度，或者只送一次的额度。"],
            ["未公布上限", "服务商没给数字，通常有速率限制。"],
            ["黄色额度", "这家可能用你的提示词训练模型，或者条款可能不允许这样用。鼠标停留可以看具体原因，不要发敏感内容。"],
          ]}
        />
        <p>
          “立即同步”和“每小时检查”会访问 OmniRoute 的目录服务并校验签名，只传递目录请求，不包含你的提示词或账号。关闭时用内置清单，不产生任何网络请求。
        </p>
      </>
    ),
  },
  {
    id: "routes",
    title: "路由",
    body: () => (
      <>
        <p>
          选中一个分组，上方的图显示它的候选账号和当前状态：哪个在用，哪个在冷却，失败后轮到谁。最近的一次请求写在图的下方。
        </p>
        <h3>内置虚拟模型</h3>
        <p>
          <code>loadout/realtime</code>、<code>loadout/fast</code>、<code>loadout/default</code> 按测速结果自动排列候选：只用超快档（首字 0.8 秒内）、快档及以上（2 秒内）、全部。默认关闭，打开开关才能用。它们可能在不同模型之间切换，要固定模型请用自己的分组。
        </p>
        <h3>测速</h3>
        <p>
          每个模型发一次很短的真实请求（让它回复一个词），会消耗一点额度。商汤这类有风控的服务商不要频繁测。测速请求在日志和总览里显示为“Loadout 测速”。
        </p>
        <h3>路由健康</h3>
        <Terms
          items={[
            ["冷却中 / 被限流", "暂时不会被选中，到时间自动恢复。"],
            ["认证失败", "密钥失效，要去账号页换密钥。"],
            ["额度耗尽", "等额度刷新，或者换账号。"],
          ]}
        />
      </>
    ),
  },
  {
    id: "retry",
    title: "重试与冷却",
    body: () => (
      <>
        <p>决定请求失败时网关怎么办，默认值可以直接用。从小到大有三层：</p>
        <Terms
          items={[
            ["先等待", "分组里的账号都在短暂冷却时，不立刻报错，先等一等再试。页面顶部的开关控制它，旁边写着最多等多久、重试几次。直接点名账号或服务商的请求不等待，立刻返回失败。"],
            ["账号冷却", "一个账号失败后休息一段时间，连续失败时间加倍。分组和服务商路由会自动跳过它。"],
            ["服务商熔断", "服务器连续报 5xx 或超时，就暂时不再尝试整个服务商，过一段时间放一个请求试探。401、403、429 只影响账号，不会触发熔断。"],
          ]}
        />
        <p>
          这一页只有“先等待”的开关可以改，其余数值只读显示。网关不会自己定时探测上游，只有客户端请求和你的点击才会访问上游。
        </p>
      </>
    ),
  },
  {
    id: "clients",
    title: "客户端与限额",
    body: () => (
      <>
        <p>
          每个客户端密钥对应一个使用 API 的软件。滑杆图标打开策略，不填就是不限制：
        </p>
        <Terms
          items={[
            ["允许的模型", "输入名称搜索，按 Tab 或点击加入，只能选已有的模型。留空表示不限。"],
            ["账号", "勾选后这个密钥只用这些账号。"],
            ["每日上限", "北京时间每天 11:00 重新计算。"],
            ["每周上限", "按滚动的最近 7 天计算。"],
            ["每月上限", "北京时间每月 1 日 08:00 重新计算。"],
            ["每分钟请求数", "超过后这一分钟内的请求被拒绝。"],
          ]}
        />
        <p>
          到达任何一个上限，这个客户端的请求返回 HTTP 429，其他客户端不受影响。费用上限按 Loadout 估算的费用计算，和服务商账单余额无关。没有价格的模型计为 0，所以价格未知时上限可能不生效。
        </p>
        <p>“吊销”立即让密钥失效，不可恢复。</p>
      </>
    ),
  },
  {
    id: "stats",
    title: "用量",
    body: () => (
      <>
      <Terms
        items={[
          ["点击柱子或热力图方块", "打开这一天的简报：请求数、成功率、费用，Token 按模型、账号、客户端的占比，以及各小时的忙闲。简报开着时可以直接点别的日子，左右方向键切换前后一天。"],
          ["热力图", "最近一年，每天一格，每月一块，颜色越深用得越多。“每周”让同一周的格子显示这周的总量，“累计”显示到当天为止的总量。"],
        ]}
      />
      <p>
        所有数字来自 Loadout 的请求记录，费用按上面说的价格顺序估算，日期按这台电脑的时区算。请求记录保留 400 天，这之前的数据不会出现。缓存相关的数字只统计上游明确报告的部分。账号的官方额度和余额在账号页。
      </p>
      </>
    ),
  },
  {
    id: "logs",
    title: "日志",
    body: () => (
      <p>
        只记录元数据：时间、客户端、模型、账号、状态、Token、耗时。提示词、回复和工具参数都不保存。点一行展开协议转换和切换记录。“TTFT”是从发出请求到收到第一个字的时间。
      </p>
    ),
  },
  {
    id: "settings",
    title: "设置和备份",
    body: () => (
      <p>
        “复制数据库”生成一份不含密钥的副本，密钥本身保存在 Windows 凭据管理器里，要带走请用账号页的导出。“重启网关”会中断进行中的请求。配色、主题和语言只影响这个窗口，配色还可以从左下角地址旁的调色板图标随时切换。“界面动效”是全局开关，关掉后页面切换、数字滚动和流向图的光点都停止，图表直接显示结果；系统设置里关闭了动画效果时也一样。
      </p>
    ),
  },
  {
    id: "trouble",
    title: "常见问题",
    body: () => (
      <Terms
        items={[
          ["软件报 404", "检查地址是否完整抄了下来，末尾是 /v1，后面不要再加别的路径。"],
          ["客户端报 401", "客户端密钥填错或已吊销。/v1 地址必须带客户端密钥，它和上游的 API 密钥不是一回事。"],
          ["客户端报 400，提示账号不在范围内", "当前子号池不包含这个账号。切回主号池，或者把账号加进子号池。"],
          ["客户端报 429", "看错误信息：可能是这个密钥到了限额，也可能是上游限流。日志页的状态一栏会写明。"],
          ["模型显示 0/0", "账号页点刷新箭头同步。如果同步报 401，就是这个账号的密钥失效了。"],
          ["总览图不动", "只有真实请求才会动。在客户端发一条，或者到路由页点测速。如果关了“界面动效”，或者系统关闭了动画效果，只有节点会亮。"],
          ["窗口关了，端口还占用着", "这是正常的，网关留在后台。从托盘退出才会停止。"],
        ]}
      />
    ),
  },
];

export function Help(props: { section?: string; gateway: string }) {
  const article = useRef<HTMLDivElement>(null);
  const [current, setCurrent] = useState(props.section ?? CHAPTERS[0]?.id);

  useEffect(() => {
    if (!props.section) return;
    setCurrent(props.section);
    document.getElementById(`guide-${props.section}`)?.scrollIntoView({ block: "start" });
  }, [props.section]);

  // The chapter being read stays marked in the list as the page scrolls.
  useEffect(() => {
    const root = document.querySelector("main");
    const node = article.current;
    if (!root || !node) return;
    const onScroll = () => {
      const top = root.getBoundingClientRect().top + 90;
      let at = CHAPTERS[0]?.id;
      for (const c of CHAPTERS) {
        const el = document.getElementById(`guide-${c.id}`);
        if (el && el.getBoundingClientRect().top <= top) at = c.id;
      }
      setCurrent(at);
    };
    root.addEventListener("scroll", onScroll, { passive: true });
    return () => root.removeEventListener("scroll", onScroll);
  }, []);

  return (
    <section className="page guide">
      <nav className="guide-toc" aria-label="使用说明目录">
        {CHAPTERS.map((c) => (
          <a key={c.id} href={`#/help/${c.id}`} className={current === c.id ? "active" : ""}>
            {c.title}
          </a>
        ))}
      </nav>
      <div className="guide-article" ref={article}>
        {CHAPTERS.map((c) => (
          <article key={c.id} id={`guide-${c.id}`}>
            <h2>{c.title}</h2>
            {c.body(props.gateway)}
          </article>
        ))}
      </div>
    </section>
  );
}
