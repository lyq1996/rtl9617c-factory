# rtl9617c_factory

RTL9617C 平台 Factory 镜像生成工具。从设备备份的 U-Boot env 分区与 `ubi_device` 容器镜像中提取产线身份与校准数据，按固定偏移输出 Factory bin，由 U-Boot 写入 UBI static 卷 `factory`。

| model | 设备 | 镜像大小 | 数据源 |
| --- | --- | --- | --- |
| `tianyi-tyfza1-2p5g` | 天邑 TYFZA1（Wi-Fi + 下行光 + FPGA） | 8 MiB | `ubi_Config` 卷内 `config_hs.xml` / `config.xml` / `rtkbosa_k.bin` / `rtkoltbosa_k.bin` / `xtemp.cal`，`ubi_fpga` 卷 |
| `youhua-pt1500-h10-ct-2p5g` | 友华 PT1500-H10（无 Wi-Fi / 无下行光） | 64 KiB | U-Boot env `kp_*` keyparam 变量 |

## 构建

```sh
cargo build --release --locked
```

## 用法

```sh
rtl9617c-factory build --model <model> \
    --env <mtd1_env.bin> [--env2 <mtd2_env2.bin>] \
    --device <mtd4_ubi_device.bin> -o <factory.bin>

rtl9617c-factory dump  --model <model> --env <...> [--env2 <...>] --device <...>
```

`build` 按机型大小输出 Factory bin，并打印各段的偏移、大小与 CRC32；`dump` 打印提取到的全部字段。

## Factory bin 规范

- 镜像按固定偏移直接排列字段，字段偏移不随机型变化。
- 镜像大小由机型决定（见上表），未使用区域填 `0xFF`。
- 多字节整数一律小端（LE）。ASCII 字符串以 NUL（`0x00`）结尾，其后保持 `0xFF`。
- 某机型不采集的字段，其区域保持 `0xFF`。

### 字段区

只收录产线烧录的硬件身份与校准数据。LOID/PLOAM/账户、产品型号、省份、默认 SSID 等属于上层软件自定义数据，不入 Factory bin。

| 偏移 | 容量 | 字段 | 类型 | tianyi-tyfza1-2p5g 来源 | youhua-pt1500-h10-ct-2p5g 来源 |
| --- | --- | --- | --- | --- | --- |
| `0x001000` | `0x10` | `BASE_MAC` | 6B MAC + 4B count | `config_hs.xml` `ELAN_MAC_ADDR`，count 固定 3 | env `kp_ucaBaseMacAddr` + `kp_ulNumMacAddrs` |
| `0x001010` | `0x06` | `EPON_MAC` | 6B MAC | `config.xml` `EPON_LLID_TBL.macAddr`（无则空） | 等于 BASE_MAC |
| `0x001020` | `0x10` | `PON_SN` | ASCII | `GPON_SN`（12 字符） | env `kp_szXponSN`（12 字符） |
| `0x001030` | `0x20` | `DEVICE_SN` | ASCII | 区域为 `0xFF` | env `kp_szSerialNumber` |
| `0x002000` | `0x1000` | `WIFI_CAL_RADIO0` | 二进制（TYFC0003） | 5G `rtl8852ce` 校准组装 | 区域为 `0xFF` |
| `0x003000` | `0x1000` | `WIFI_CAL_RADIO1` | 二进制（TYFC0003） | 2G `rtl8192xbe` 校准组装 | 区域为 `0xFF` |
| `0x004000` | `0x0800` | `BOSA_CALIBRATION` | 二进制 | `rtkbosa_k.bin`（1024B） | 区域为 `0xFF` |
| `0x004800` | `0x0400` | `BOSA_OLT_CALIBRATION` | 二进制 | `rtkoltbosa_k.bin`（588B） | 区域为 `0xFF` |
| `0x005000` | `0x1000` | `XTEMP_CALIBRATION` | JSON 文本 | `xtemp.cal` | 区域为 `0xFF` |
| `0x010000` | `0x7E0000` | `FPGA_IMAGE` | 二进制 | `ubi_fpga` 卷原始内容（fpgafs squashfs，4317184B） | 区域为 `0xFF` |

### flash-calibration.bin（TYFC0003）

Wi-Fi 校准段为原厂 `factory.uc` 组装格式，字段取自 `config_hs.xml`：

1. 魔数 `TYFC0003`（8B）。
2. 5 个 RX gaingap 单字节：`HW_RX_GAINGAP_{2G_CCK,2G_OFDM,5GL,5GM,5GH}_AB`，十进制字符串转字节。
3. 每 path A/B 的 `HW_THERMAL_{A,B}_WLAN{n}`（2B）与 `HW_XCAP_WLAN{n}_AX`（1B），十进制。
4. band × path 的 TSSI hex 数组：`HW_{2G_CCK,2G_BW40_1S,5G_BW40_1S}_TSSI_{A,B}`，逗号分隔 hex。
5. 6B MAC：BASE_MAC 整体加 `2 - radio_index`（5G wlan0 为 +2，2G wlan1 为 +1）。
6. `HW_2G_CCK_TSSI_{GAIN,CW}_DIFF_{A,B}` 与 `HW_{2G,5G}_TSSI_{GAIN,CW}_DIFF_{A,B}` hex 数组。
7. 尾部 112B：种子 `0` 与 `HW_5G_TSSI_CW_DIFF_A` 最后一字节，各重复 4 个 width × 14B，其中 width 1 全 0。

## 数据提取说明

- **U-Boot env**：`crc32(4B LE) + flags(1B) + data`；`data` 为 `name=value\0` 列表，CRC 校验 zlib 式。多分区（env/env2）均校验，取 flags 更大者。`kp_*` 即产线 keyparam。
- **ubi_device 容器**：按 EC 头 `vid_hdr_offset`/`data_offset` 解析，每 PEB 校验 EC/VID CRC，同 `(vol_id, lnum)` 取 `sqnum` 较新者；卷表（vol_id `0x7FFFEFFF`）定位卷名。dynamic 卷未映射（free）的 LEB 以 `0xFF` 补齐到 `reserved_pebs`，不影响 UBIFS 提交索引。
- **UBIFS**：从 superblock / master（LEB 1、2 取 commit 最新）沿 TNC 提交索引提取 `config_hs.xml`、`config.xml`、校准文件。压缩支持 none / LZO / zlib（raw deflate）。
- **ubi_fpga**：卷原始内容直接搬移（洞补 `0xFF`），消费端整体写回对应卷即可。

## U-Boot 刷入约定

bin 的内容整体写入 UBI static 卷 `factory`（`ubiupdatevol` 或 U-Boot `ubi write`），写入长度即上表中的机型镜像大小。

## 扩展新机型

在 `src/profile.rs` 的 `MODELS` 中追加 `Model`（名称、镜像大小、提取函数）；需要新字段时在 `src/layout.rs` 的 `FIELDS` 中追加偏移与容量。

## 已知限制

- `tianyi-tyfza1-2p5g` 无整机 SN 的产线数据源，`DEVICE_SN` 为空。
- LOID/PLOAM/OMCI 密钥、产品型号、省份、默认 SSID/密码等上层自定义数据不入 Factory bin，由上层软件从原配置自行管理。
