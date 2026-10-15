# Modular MOBA · 模块化像素 MOBA

一个用 **Rust + WebAssembly** 写的像素风 MOBA（monorepo）。客户端运行在浏览器里（Rust 编译成 wasm，Canvas 渲染），服务端是原生 Rust 的权威服务器。地图结构和操作方式对齐《英雄联盟》。

核心玩法是 **"模块化英雄"**：

- 一共 6 个 **定位**：坦克 Tank、战士 Fighter、刺客 Assassin、法师 Mage、射手 Marksman、辅助 Support。
- 每个 **英雄** 有两个定位，并自带一个 **R（大招）**。
- 每个 **技能** 也有两个定位。玩家从技能池里给英雄挑三个技能放进 **Q / W / E**。
- 规则：**Q/W/E 的每个技能都必须和英雄至少共享一个定位**。客户端选人界面会实时校验，服务端开局前会再校验一次（不合法会被拒绝并修正）。

```
铁甲 Ironclad（坦克/战士）
  Q 盾击冲锋 Shield Bash   坦克/战士 ✔ 共享 坦克、战士
  W 击退射击 Repel Shot    坦克/射手 ✔ 共享 坦克
  E 旋风刃   Whirling Blades 战士/刺客 ✔ 共享 战士
  ✘ 治疗波   Healing Wave  法师/辅助   没有共同定位，不能装备
  R 裂地 Earthshatter（英雄自带）
```

## 快速开始

需要：Rust（stable，≥ 1.87）。脚本会自动安装 `wasm32-unknown-unknown` target 和版本匹配的 `wasm-bindgen-cli`。

```bash
# 1. 编译浏览器客户端（输出到 web/pkg）
./scripts/build-client.sh

# 2. 启动服务器（同时托管 web/ 目录）
cargo run --release -p moba-server

# 3. 打开浏览器
open http://localhost:8080
```

- **PRACTICE VS BOTS（离线练习）**：整局游戏（9 个 AI + 你）直接在浏览器的 wasm 里模拟，不需要服务器（只要能访问到静态页面即可）。
- **PLAY ONLINE（联机）**：同一个房间名的玩家进同一个大厅（10 个座位，蓝/红各 5 个，座位顺序 = 上 / 打野 / 中 / 下 / 辅助）。房间名可以写在网址里分享，例如 `http://<host>:8080/#myroom`。房主点 START GAME 开局，空位由 AI 补齐；中途掉线的玩家由 AI 接管。比赛进行中才进房间的人会在大厅等下一局（不提供实时观战，避免有人开小号给队友报视野）。

服务器参数：`--port 9000` 或环境变量 `PORT`；`MOBA_WEB_DIR` 指定静态文件目录（默认 `./web`）。

## 操作（对齐 LoL）

| 按键 | 作用 |
| --- | --- |
| 右键 | 移动 / 攻击目标（按住右键持续移动） |
| A + 左键 | 攻击移动（A-move） |
| S | 停止 |
| Q W E R | 释放技能：按下显示指示器，松开施放；自身技能按下立即施放 |
| Alt/Ctrl + Q W E R（或点技能上方的 +） | 升级技能（R 在 6/11/16 级解锁；浏览器里 Ctrl+W 会关闭标签页，建议用 Alt） |
| D / F | 召唤师技能（闪现、治疗、幽灵疾步、点燃、虚弱、屏障） |
| B | 回城（8 秒，受到伤害会被打断） |
| P | 商店（只能在泉水或死亡时购买；右键背包物品出售） |
| 1–6 | 使用物品（生命药水） |
| Tab（按住） | 计分板 |
| Y | 锁定 / 解锁镜头；空格（按住）回到英雄；鼠标移到屏幕边缘或方向键平移镜头 |
| 小地图 | 左键移动镜头，右键移动英雄 |
| Esc | 菜单（离线模式下会暂停） |

## 地图与规则

地图（3200×3200 单位）按召唤师峡谷的结构生成，中心对称：

- 上 / 中 / 下三路，每路 外塔 → 内塔 → 高地塔 → 水晶（兵营），水晶前两座门牙塔，最后是基地水晶（Nexus）。
- 防御塔保护顺序与 LoL 相同（外塔没倒之前内塔无敌……两座门牙塔都倒了基地才能被攻击），并带有偷塔保护：没有己方小兵在场时英雄对建筑伤害大幅降低。
- 防御塔：优先打小兵，有敌方英雄在塔下攻击己方英雄时立刻转火该英雄，连续命中同一英雄伤害递增。
- 兵线：每 30 秒一波（3 近战 + 3 法师，每第 3 波有炮车），对方水晶被拆的路出超级兵；小兵会响应"英雄攻击英雄"的呼救。
- 野区：蓝 BUFF、红 BUFF、F6/石甲虫/狼/蛤蟆（像素版），河道上方大龙坑（Baron），下方小龙坑（Dragon）。蓝/红 BUFF 给击杀者增益，小龙给全队永久增伤，大龙给全队临时属性。
- 战争迷雾：每队只能看到己方单位视野内的东西，墙壁阻挡视野；草丛中的单位只对同一片草丛里的敌人可见（攻击会短暂暴露）。服务端只把对方队伍能看到的东西发给客户端，所以不能通过改客户端开图。
- 等级 1–18、经验共享、补刀金币、击杀/助攻/一血奖励、复活时间随等级增长、泉水回血和泉水激光。

## 英雄（10 个）

| 英雄 | 定位 | 自带 R |
| --- | --- | --- |
| Ironclad 铁甲 | 坦克 / 战士 | Earthshatter 裂地：蓄力后击飞周围敌人 |
| Aegis 神盾 | 坦克 / 辅助 | Sanctuary 圣域：给附近所有队友护盾并大幅减速敌人 |
| Colossus 巨像 | 坦克 / 法师 | Unstoppable Force 势不可挡：冲向目标点，落地击飞 |
| Kaelen 凯伦 | 战士 / 刺客 | Execution 处决：跳向敌方英雄，按已损失生命追加伤害 |
| Valkyr 瓦尔基里 | 战士 / 辅助 | Skyfall 天降神兵：引导后从天而降到远处 |
| Shade 暗影 | 刺客 / 法师 | Void Rift 虚空裂隙：闪烁并沉默落点敌人 |
| Sable 黑貂 | 刺客 / 射手 | Shadow Rush 影袭：穿过路径上所有敌人 |
| Vex 维克丝 | 射手 / 法师 | Arc Cannon 奥术巨炮：全图穿透巨炮 |
| Lumen 流明 | 法师 / 辅助 | Radiant Beam 光辉射线：伤害敌人、治疗队友的光束 |
| Hawk 鹰眼 | 射手 / 战士 | Deadeye 致命狙击：瞄准后必中的超远距离狙击 |

## 技能池（24 个，每个定位恰好出现在 8 个技能上）

| 技能 | 定位 | 类型 |
| --- | --- | --- |
| Shield Bash 盾击冲锋 | 坦克/战士 | 冲锋，撞到的第一个敌人被眩晕 |
| Ground Slam 大地震击 | 坦克/战士 | 自身范围伤害 + 减速 |
| Taunting Roar 挑衅怒吼 | 坦克/辅助 | 范围嘲讽 + 自身双抗 |
| Guardian Link 守护链接 | 坦克/辅助 | 给队友和自己护盾 |
| Seismic Shard 地震碎片 | 坦克/法师 | 指向弹道，减速目标并给自己加速 |
| Stone Skin 石肤术 | 坦克/法师 | 按最大生命值的护盾 + 护甲 |
| Hook 钩锁 | 坦克/刺客 | 钩子，把第一个敌人拉到身边 |
| Repel Shot 击退射击 | 坦克/射手 | 击退，撞墙眩晕 |
| Lunge 突刺 | 战士/刺客 | 指向突进 |
| Whirling Blades 旋风刃 | 战士/刺客 | 旋转斩 + 回复 |
| Rallying Cry 集结号令 | 战士/辅助 | 团队移速 + 攻速 |
| Tumble 翻滚 | 战士/射手 | 短位移，强化下一次普攻 |
| Rapid Fire 急速射击 | 战士/射手 | 大幅攻速 |
| Infernal Cleave 炼狱横扫 | 战士/法师 | 扇形伤害 + 减速 |
| Shadow Blink 暗影闪烁 | 刺客/法师 | 闪烁 + 落点爆炸 |
| Void Bolt 虚空飞弹 | 刺客/法师 | 高速技能射击 |
| Piercing Arrow 穿透之箭 | 刺客/射手 | 穿透直线箭 |
| Marked for Death 死亡印记 | 刺客/射手 | 目标承受伤害提高 |
| Smoke Bomb 烟雾弹 | 刺客/辅助 | 致盲 + 减速，队友加速 |
| Healing Wave 治疗波 | 法师/辅助 | 范围治疗友军、伤害敌人 |
| Frost Nova 冰霜新星 | 法师/辅助 | 延迟后禁锢 |
| Mystic Shot 秘术射击 | 法师/射手 | 远程技能射击（AD+AP 加成） |
| Volley 齐射 | 射手/辅助 | 扇形箭雨 + 减速 |
| Snare Trap 捕兽夹 | 射手/辅助 | 放置陷阱，禁锢踩中的英雄 |

## 仓库结构

```
Cargo.toml                Cargo workspace（monorepo）
crates/
  core/    moba-core      客户端和服务端共享的一切
    src/roles.rs            定位
    src/skills.rs           技能池 + 大招（纯数据）
    src/heroes.rs           英雄（定位、属性、外观参数、大招）
    src/loadout.rs          "Q/W/E 必须和英雄共享定位" 规则
    src/items.rs            商店物品
    src/summoners.rs        召唤师技能
    src/units.rs            小兵 / 野怪 / 建筑数值
    src/map.rs              峡谷地图生成 + A* 寻路
    src/protocol.rs         网络消息（postcard 二进制编码）
    src/sim/                确定性 30Hz 模拟
      mod.rs                  World、指令、出兵、刷野、复活
      combat.rs               伤害、死亡、奖励、普攻、弹道、泉水
      abilities.rs            技能施放 / 效果系统 / 区域 / 召唤师技能 / 商店
      ai.rs                   小兵、防御塔、野怪、英雄指令
      movement.rs             移动、冲刺、击退、碰撞
      vision.rs               战争迷雾 + 草丛
      snapshot.rs             按玩家视野生成快照
      bot.rs                  英雄 AI（和玩家一样只通过 Command 操作）
  server/  moba-server    axum WebSocket：房间、大厅、开局、权威模拟、快照下发
  client/  moba-client    wasm 客户端
    src/app.rs              主菜单 / 选人 / 大厅 / 对局切换
    src/game.rs             快照插值、特效、LoL 操作 → Command、离线模拟
    src/render.rs           世界渲染（地形、单位、弹道、特效、迷雾、血条）
    src/hud.rs              HUD、小地图、商店、计分板、结算
    src/art.rs              程序化像素美术（英雄由外观参数组装）
    src/terrain.rs          程序化像素地形
    src/font.rs             5x7 像素字体
    src/gfx.rs / ui.rs      Canvas 封装、即时模式 UI
web/index.html            页面入口（构建产物放在 web/pkg，不提交）
scripts/build-client.sh   构建客户端
```

### 架构要点

- **一份模拟，两处运行**：`moba-core::sim::World` 是确定性的固定步长（30Hz）模拟。联机时它跑在服务器上（权威），离线练习时同一份代码编译成 wasm 跑在浏览器里。客户端两种模式都只消费 `Snapshot`，渲染路径完全一样。
- **数据驱动的技能系统**：技能 = `Targeting`（施放方式）+ `Action`（形状：弹道、范围、扇形、光束、冲刺、闪烁、指向、陷阱）+ 若干 `Effect`（伤害、治疗、护盾、减速、眩晕、禁锢、击飞、沉默、嘲讽、致盲、击退、拉拽、增益、易伤、强化普攻）。模拟层只解释这些积木，所以**新增一个技能只需要在 `SKILLS` 里加一条数据**，不用写新逻辑；技能说明、图标、指示器也都从数据自动生成。
- **新增英雄**：在 `HEROES` 里加一条（两个定位 + 属性 + 外观参数 + 一个新的大招技能），像素形象会根据外观参数（配色、头饰、武器、披风、体型）自动绘制。
- **网络**：WebSocket 二进制帧 + postcard 编码。服务端每 tick 为每个玩家单独生成快照（只含该队视野内的内容），客户端在两帧之间插值显示。
- **AI**：机器人只能看到自己队伍的视野，通过和玩家相同的 `Command` 接口操作（对线补刀、推塔、回城买装备、打野会先估算能不能打过野怪、残血撤退、使用技能和召唤师技能）。

## 开发

```bash
cargo test --workspace --release                      # 全部测试（含 10 AI 完整对局的稳定性测试）
cargo clippy --workspace --all-targets
cargo clippy -p moba-client --target wasm32-unknown-unknown

# 无界面跑一局 5v5 AI 对局，每分钟打印战况（seed、时长、rot=N 轮换英雄分路、causes 打印死亡原因）
cargo run --release -p moba-core --example bot_match -- 7 40

# 导出地图预览 / 像素美术预览（PPM 图片）
cargo run -p moba-core --example map_preview -- map.ppm
cargo run -p moba-client --example art_preview -- .
```

调试参数：离线模式下在网址后加 `?speed=10` 可以让模拟加速 10 倍（例如 `http://localhost:8080/?speed=10`）。
