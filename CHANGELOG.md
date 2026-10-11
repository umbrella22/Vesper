# Changelog

## Unreleased

## 0.9.1 - 2026-10-12

<!-- release-notes:en -->

### Fixed

- Recover the original iOS DASH resource failure when AVFoundation copies an
  SDK error into a plain `NSError`. Preserve the network category, original
  message, retry policy and DASH diagnostics after checking the current item,
  session and source load epoch; reject unrelated and stale error matches.
- Restore Linux CLI fixture compilation, run iOS screen-sleep regressions in
  an application host, and synchronize cache cleanup, route-error delivery and
  Android relay-close assertions with their actual completion callbacks.
- Complete Plugin SDK release recovery after test-only build repairs, with
  validated tag ancestry, production-input identity and explicit original-tag
  and artifact-source provenance.

<!-- release-notes:zh-CN -->

### 修复

- AVFoundation 将 SDK 错误复制为普通 `NSError` 后，恢复原始 iOS DASH 资源
  失败。校验当前 item、session 和 source load epoch 后保留网络分类、原始
  消息、重试策略与 DASH 诊断；拒绝无关错误及过期来源的匹配。
- 修复 Linux CLI 素材辅助函数的编译，在应用宿主中执行 iOS 屏幕休眠回归，
  并根据实际完成回调同步缓存清理、路由错误交付和 Android relay 关闭断言。
- 支持测试构建修复后的完整 Plugin SDK 发布恢复，校验 tag 祖先关系及生产
  输入身份，明确记录原始 tag 和制品构建来源。

## 0.9.0 - 2026-10-12

<!-- release-notes:en -->

### Changed

- **Breaking (Native Rust author SDK):** EventHook and BenchmarkSink authors now
  implement `PluginEventHook` and `PluginBenchmarkSink`, returning
  `PipelineEventHookFailure` and `BenchmarkSinkFailure`. Export builder method
  names remain unchanged. Checked host traits, Native ABI/WIT contracts, and
  mobile public DTOs retain their existing forms. Follow the
  [author API migration guide](lib/doc/plugin-author-api.md).

- **Breaking (WASM host Rust APIs):** replace ambiguous synchronous queue calls
  with accepted-work tickets and `submit_and_wait`; add correlated reports and
  cumulative flush/close settlement. Waiting expiry and dropped receipts leave
  accepted work running. Preserve typed Wasmtime causes, error provenance, and
  quarantine reasons; distinguish queue failures from caller wait expiry.
- Validate owned WASM queue input once through immutable checked values, share
  duplicate-rejecting attribute decoding across generated worlds, and remove
  the unused legacy runtime event mapper. Existing protocol/resource budgets
  and native-first Media3/AVPlayer routes remain unchanged.
- Raise the Rust toolchain and MSRV to 1.99.0 and align plugin scaffolds and CI.
  Refresh the shared Rust dependency set and FFmpeg source/build policy.

### Fixed

- Refresh FFmpeg, Android OpenSSL, and Android libxml2 patch selection against
  upstream release indexes even with populated source caches. Cache new
  versioned archives and rebuild dependency installs and FFmpeg outputs when
  source or patch identity changes; retain cached patches as an offline
  fallback and exact-version overrides for reproduction.
- Preserve checked host failure categories and full diagnostics in download
  event reports. Bound only Android/iOS outward report messages on UTF-8
  boundaries, retaining the established codes and public DTO schema.
- Settle active and queued receipts after WASM worker panic; keep worker-time
  flush snapshots and persist guest-flush quarantine causes. Bounded report loss
  remains observable independently of invocation settlement.
- Reject segmented exports when every compatible post-download processor skips;
  return `Unsupported/Capability` and preserve the source manifest and target
  file. Export remains available after a processor produces a real single-file
  output.
- Preserve exact integer catalog revisions at the iOS Flutter MethodChannel
  boundary. Reject Boolean, fractional, non-finite, and out-of-range values
  without lossy `NSNumber` conversion.
- Preserve HLS resource identity, byte ranges and initialization maps, and
  inherited DASH BaseURLs and template timing in download planning.
- Propagate native DASH rendition and download recovery failures, and preserve
  cancellation through recovery, preparation and resource handoff.
- Reject malformed Flutter caller policies and download graphs with explicit
  channel errors. Release native managers and sequence attachments when
  construction or registration fails.
- Preserve preload capacity through physical cancellation, check aggregate
  staging budgets, and fence stale activation and source-session callbacks.
- Restore native surface lifecycle and screen-sleep behavior, retain current
  first-frame and track observations, and release detached native-frame leases.
- Support bounded Android DLNA `SegmentTimeline` relay planning through the
  shared Rust DASH bridge, including original `$Time$` ticks and finite negative
  repeats. Correct the FFmpeg AAC probe profile used for MP4 remux.
- Preserve terminal FFmpeg demux read errors as failures, remove failed staged
  remux output without replacing an existing destination, and report progress
  completion only after publication.
- Propagate Android external-playback cancellation through open waits, FIFO
  FFmpeg workers, HLS/cache stream handles, and generation-checked native
  session leases. Retire all relay token lifetimes before blocking cleanup and
  make adapted input teardown idempotent.
- Configure the Maven signing provider with an empty password when an
  unencrypted signing key has no passphrase, so valid key material still
  creates a signatory.
- Resolve iOS optional-plugin acceptance projects from their isolated directory,
  preserving original Swift and fixture paths and verified framework identity.
  Nested output directories now build the Release device test host correctly.

<!-- release-notes:zh-CN -->

### 变更

- **破坏性变更（Native Rust 作者 SDK）：** EventHook 和 BenchmarkSink 作者现在
  实现 `PluginEventHook` 和 `PluginBenchmarkSink`，返回
  `PipelineEventHookFailure` 与 `BenchmarkSinkFailure`。导出 builder 方法名称
  保持不变；checked host traits、Native ABI/WIT 契约及移动端公开 DTO 形式保持
  不变。迁移方法见[作者 API 迁移指南](lib/doc/plugin-author-api.md)。
- **破坏性变更（WASM host Rust API）：** 使用 accepted-work ticket 和
  `submit_and_wait` 替代语义不明确的同步队列调用；增加关联报告和累计
  flush/close 结算。等待超时和丢弃 receipt 不会停止已接受的工作；保留类型化
  Wasmtime 原因、错误来源和隔离原因，并区分队列失败与调用方等待超时。
- 通过不可变 checked 值一次性校验自有 WASM 队列输入，在生成的 world 之间共享
  拒绝重复属性的解码逻辑，并移除未使用的旧运行时事件映射。现有协议和资源预算
  以及 native-first Media3/AVPlayer 路径保持不变。
- 将 Rust 工具链及 MSRV 提升至 1.99.0，同步插件模板与 CI，并更新共享 Rust
  依赖及 FFmpeg 源码和构建策略。

### 修复

- 即使源缓存已填充，也根据上游发布索引刷新 FFmpeg、Android OpenSSL 和
  Android libxml2 补丁选择。缓存新版本归档，并在源或补丁身份变化时重建依赖
  安装和 FFmpeg 输出；保留缓存补丁作为离线回退和精确版本覆盖。
- 在下载事件报告中保留已检查的宿主失败类别和完整诊断信息。仅在 UTF-8 边界
  截断 Android/iOS 向外报告的消息，保留既有代码和公开 DTO schema。
- WASM worker panic 后结算活动和排队 receipt；保留 worker-time flush 快照并
  持久化 guest-flush 隔离原因。报告有界丢失独立于 invocation settlement 保持
  可观测。
- 当分段下载后的所有兼容处理器都跳过时，不再把 manifest 导出为 MP4；返回
  `Unsupported/Capability` 并保留源文件和目标文件。处理器实际生成单文件输出
  后仍允许导出。
- 在 iOS Flutter MethodChannel 边界保留 NSNumber 整数 catalog revision 的精确
  值；拒绝 Boolean、小数、非有限数和越界输入，避免有损转换。
- 下载规划保留 HLS 资源身份、字节范围和初始化映射，以及继承的 DASH BaseURL
  与模板时间信息。
- 传播原生 DASH rendition 和下载 recovery 失败，保留 recovery、准备及资源
  交接过程中的取消语义。
- 对畸形 Flutter 调用策略及下载 graph 返回明确的通道错误，并在构造或注册
  失败时释放原生下载 manager 和序列绑定。
- 在实际取消工作退出前保留预加载容量，检查累计暂存预算，并隔离过期激活和
  来源会话回调。
- 修复原生 Surface 生命周期与屏幕休眠行为，保留当前首帧和轨道观测，并释放
  已脱离的 native-frame 租约。
- Android DLNA 的有界 `SegmentTimeline` relay 通过共享 Rust DASH bridge 规划，
  保留原始 `$Time$` ticks 与有限负重复；修正 MP4 remux 的 FFmpeg AAC 探测 profile。
- FFmpeg 终态 demux 读取错误现在保持为失败；失败的 remux 暂存文件会被清理且
  不替换已有目标，只有发布成功后才报告完成进度。
- Android external-playback 的取消语义贯穿等待、FIFO FFmpeg worker、HLS/cache
  stream handle 和带代际校验的 native session lease；relay token 会先全部退役，
  再执行可能阻塞的清理，adapted input 的释放具备幂等性。
- Maven 签名在非加密密钥未提供口令时使用空字符串初始化，确保创建有效的签名者。
- iOS optional-plugin 验收项目现在以隔离目录解析路径，保留原始 Swift 源码、
  素材及已验证 framework 的身份，修复嵌套输出目录中的 Release 真机宿主构建。

## 0.8.0 - 2026-09-29

<!-- release-notes:en -->

### Added

- Add independent source sessions with immutable registrations, opaque handles,
  bounded preload tasks and explicit cache-reuse capabilities. Local MPDs can
  preload their absolute HTTPS media without allocating a player.
- Add awaited handle activation with explicit play intent, start position, rate
  and timeout, plus activation/source identities and native playback epochs.
- Preserve acquired playback and sequence leases on ordinary registration close;
  reject new activation after access invalidation or monotonic source expiry.

### Changed

- **Breaking:** sequence items and provider responses now carry source handles.
  Native hosts assign source revisions and cache identity; the old Flutter
  cache-identity and resolved-source DTOs are removed.
- **Breaking:** list replacement, append/prepend, removal, reorder and resolved
  source acceptance never implicitly activate playback. Call `activate`, `next`
  or `previous` explicitly. Paging boundaries return an optional empty result.
- Use one native activation path for ordinary and sequence playback. Upgrade
  matching native/Flutter packages together and follow the
  [0.7-to-0.8 migration guide](lib/doc/source-lifecycle.md).

### Fixed

- Keep worker slots and staging reservations occupied until physical preload
  work exits, even after timeout/cancellation; include queue time in deadlines.
- Preserve revisions on same-handle reorder, fence pending activation after
  list changes, retain preload results per registered source, and reject lossy
  native-channel integer decoding.
- Terminate iOS sequence JSON buffers correctly at the C FFI boundary.
- Consume source-session startup bytes independently of controller cache policy,
  including local MPDs whose media lives on HTTPS origins.
- Use the preloaded, validated media redirect target for later iOS SegmentBase
  range reads, while retaining the original source identity for cache lookup.

<!-- release-notes:zh-CN -->

### 新增

- 新增独立来源会话、不可变来源注册、不透明句柄、有界预加载任务与明确的缓存
  复用能力。无需创建播放器即可预加载本地 MPD 引用的 HTTPS 媒体。
- 新增可等待的句柄激活，显式设置播放意图、起始位置、倍速与超时，并返回
  激活标识、来源标识及原生播放代次，供后续诊断关联。
- 普通关闭保留已取得的播放及序列租约；访问失效或来源过期后拒绝新的激活。

### 变更

- **破坏性变更：** 序列条目和解析结果改为来源句柄；来源版本与缓存身份由
  原生 SDK 分配，移除旧 Flutter 缓存身份和已解析来源 DTO。
- **破坏性变更：** 列表替换、补页、删除、重排和接受解析结果均不隐式播放。
  宿主显式调用激活或导航；到达补页边界时返回空结果。
- 普通播放与序列共用原生激活路径；原生和 Flutter 包应同步升级，迁移方法见
  [0.7 至 0.8 指南](lib/doc/source-lifecycle.md)。

### 修复

- 超时或取消后，工作槽位与暂存预算保留至实际任务退出；预加载期限包含排队时间。
- 保留相同句柄重排后的来源版本，正确终结失效激活，按注册来源保留预加载结果，
  并拒绝原生通道对小数或越界整数的有损转换。
- 修正 iOS 序列 JSON 在 C FFI 边界缺少字符串终止符的问题。
- 来源会话预加载缓存的消费独立于播放器缓存策略，本地 MPD 引用 HTTPS 媒体时同样生效。
- iOS SegmentBase 后续范围使用预加载时验证过的媒体跳转目标，缓存查询仍保留原始来源身份。

## 0.7.0 - 2026-09-28

<!-- release-notes:en -->

### Added

- Retain native audio input evidence and monotonic first-frame observations per
  playback attempt, including typed Flutter warnings and terminal-error context.
- Observe suspected playback stalls with configurable native timers and retained
  evidence. Add audio-decoder capability probes with explicit supported,
  unsupported and unknown outcomes; iOS reports its unavailable decoder query.
- Warm static DASH SegmentBase MPD, index, initialization and first-media ranges
  through playback sequences, with bounded caches consumed by Media3 and the
  iOS AVPlayer DASH bridge. Preserve source/credential isolation, cancellation
  fences, byte budgets and formal playback fallback.

### Changed

- **Rust migration:** `SequenceResolvedSource` literals now require
  `warmup_goal`; `SequenceWarmupGoal` includes `DashSegmentBaseStartup`.
  See the [startup cache contract](lib/doc/dash-startup-cache.md) for migration,
  cache limits and the distinction between warmup completion and first-frame
  evidence. Native and Flutter consumers should upgrade matching host packages.

<!-- release-notes:zh-CN -->

### 新增

- 按播放尝试保留原生音频输入证据与单调时钟首帧观测，提供 Flutter 类型化
  警告，并将诊断上下文保留到终态错误。
- 使用可配置的原生定时器观测疑似播放停滞并保留证据；新增音频解码能力
  探测，明确区分支持、不支持与未知，iOS 显式报告系统查询能力缺失。
- 播放序列支持静态 DASH SegmentBase 的 MPD、索引、初始化段及首个媒体段
  预热，Media3 与 iOS AVPlayer DASH 桥接实际复用有界缓存，并保持来源与鉴权
  隔离、取消屏障、字节预算及正式播放回退。

### 改进

- **Rust 迁移：** `SequenceResolvedSource` 字面量新增必填 `warmup_goal`；
  `SequenceWarmupGoal` 新增 `DashSegmentBaseStartup`。迁移方法、缓存边界与
  预热完成和首帧证据的区别见 [启动缓存契约](lib/doc/dash-startup-cache.md)。
  原生与 Flutter 使用方应同步升级匹配的宿主包。

## 0.6.5 - 2026-09-28

<!-- release-notes:en -->

### Fixed

- Normalize Android buffering overrides at the Media3 boundary so playback
  thresholds above the minimum buffer no longer prevent initialization.
- Compute Android and iOS retry backoff with bounded integer arithmetic,
  preserving zero and large delays without overflow, floating-point rounding,
  or NaN failures. Saturate iOS sleep conversion at the nanosecond limit.
- Infer DASH adaptation types from Representation MIME types and inherited
  codecs when the AdaptationSet type is unknown. Preserve URL query strings,
  fragments, and empty path segments when resolving relative BaseURLs.
- Parse DASH durations containing days or zero-valued calendar fields, and
  decode XML character references exactly once in resource URLs and metadata.

<!-- release-notes:zh-CN -->

### 修复

- 在 Android Media3 边界规范缓冲参数，避免起播或重新缓冲阈值高于最小缓冲
  时导致播放器初始化失败。
- Android 和 iOS 重试退避改用有界整数运算，正确处理零延迟和大数，避免
  溢出、浮点舍入及 NaN 异常；iOS 休眠时间转换在纳秒表示上限处饱和。
- 当 AdaptationSet 类型未知时，从 Representation MIME 类型及继承的 codecs
  推断 DASH 轨道类型；解析相对 BaseURL 时保留查询参数、片段和空路径段。
- 支持包含天数或零值年月字段的 DASH 时长，资源 URL 和元数据中的 XML
  字符引用仅解码一次。

## 0.6.4 - 2026-09-21

<!-- release-notes:en -->

### Added

- Added configurable Stage skins to Flutter, Android Compose, and iOS SwiftUI:
  semantic icon roles, custom icon views, colors, button variants, and
  timeline/HUD styling. Hosts can switch skins at runtime or select SDK defaults
  with null/nil while retaining their playback surface.
- Exposed reusable skinned action buttons in the native UI modules and forwarded
  skin selection through all three example Stage wrappers.

### Changed

- **Breaking:** Flutter `VesperStageIconButton.icon` now takes a `Widget`.
  Button sizing parameters move to `VesperStageButtonStyle` and named variants.
  Native UI consumers must rebuild for the extended Stage signatures. The
  [0.6.4 migration guide](https://github.com/umbrella22/Vesper/blob/v0.6.4/lib/doc/stage-skins.md)
  includes per-platform examples and the button API migration.
- Updated the first-party Vesper Player Agent with skin API guidance, migration
  rules, UI boundaries, and platform-specific validation entrypoints.

### Fixed

- Excluded custom SwiftUI icon descendants from the accessibility tree while
  preserving SDK button labels, actions, and minimum hit areas. Decorative
  scrims, borders, and HUD content now pass Stage gestures through.

<!-- release-notes:zh-CN -->

### 新增

- Flutter、Android Compose 和 iOS SwiftUI 播放器支持皮肤配置，包括语义图标、
  自定义图标视图、配色、按钮样式以及时间轴和手势 HUD 样式。宿主可以在运行时
  切换皮肤，或通过 null/nil 恢复 SDK 默认外观，同时保留播放 Surface。
- 原生 UI 模块提供可复用的皮肤按钮，三个平台的示例 Stage 均支持传入皮肤。

### 改进

- **破坏性变更：** Flutter `VesperStageIconButton.icon` 改为接收 `Widget`，
  按钮尺寸参数迁移至 `VesperStageButtonStyle` 和命名样式。原生 UI 使用方需要
  针对扩展后的 Stage 签名重新编译。
  [0.6.4 迁移指南](https://github.com/umbrella22/Vesper/blob/v0.6.4/lib/doc/stage-skins.md)
  提供各平台接入示例和按钮 API 迁移步骤。
- 第一方 Vesper Player Agent 同步更新皮肤 API、迁移规则、UI 模块边界和各平台
  验证入口。

### 修复

- 自定义 SwiftUI 图标的子节点不再泄漏到无障碍树，按钮保留 SDK 标签、动作及
  最小触摸热区。渐变遮罩、边框和手势 HUD 装饰内容不再拦截 Stage 手势。

## 0.6.3 - 2026-09-20

<!-- release-notes:en -->

### Fixed

- Reject unsupported Android architectures before player JNI initialization,
  preserving actionable errors across repeated creation attempts. Flutter
  reports `VesperUnsupportedError` with platform code
  `vesper_unsupported_architecture` and runtime ABI details.
- Clarified the Android arm64-only requirement in native and Flutter package
  documentation and Flutter package descriptions.

<!-- release-notes:zh-CN -->

### 修复

- 在播放器 JNI 初始化前拒绝不支持的 Android 架构，重复创建仍返回明确错误。
  Flutter 通过 `VesperUnsupportedError` 返回平台错误码
  `vesper_unsupported_architecture` 和运行时 ABI 信息。
- 在 Android、Flutter 包文档及 Flutter 包描述中明确仅支持 arm64 的要求。

## 0.6.2 - 2026-09-18

<!-- release-notes:en -->

### Fixed

- Preserved portrait video aspect ratios in Android Compose manual and
  automatic Picture in Picture using display dimensions, decoder observations,
  and selected-track metadata.
- Preserved iOS SwiftUI playback when a temporary fullscreen surface replaces
  and later restores a retained inline surface, including portrait geometry.
- Synchronized Flutter iOS Picture in Picture configuration with the native
  controller, preventing unwanted background re-entry after automatic entry
  is disabled while preserving explicit manual entry.
- Cancelled pending Flutter iOS Picture in Picture starts after exit, surface
  replacement, or session disposal, and rechecked system readiness before
  executing queued requests.

### Changed

- GitHub Release change summaries now use the matching version in the tagged
  `CHANGELOG.md`, with separate English and Simplified Chinese content instead
  of abbreviated commit titles. Missing translations fail note generation.

<!-- release-notes:zh-CN -->

### 修复

- Android Compose 手动和自动进入画中画时，会根据视频显示尺寸、解码器观测值和
  已选轨道元数据保持竖屏视频比例。
- iOS SwiftUI 临时全屏容器关闭后，会恢复保留的内嵌播放画面，竖屏视频也能正常恢复。
- Flutter iOS 画中画配置会同步到系统控制器；关闭自动进入后不会在退到后台时意外
  重新进入画中画，同时保留显式手动进入能力。
- Flutter iOS 退出画中画、更换播放视图或销毁会话时，会取消等待中的画中画请求；
  执行排队请求前会重新检查系统就绪状态。

### 改进

- GitHub Release 的变更摘要改为读取发布标签中 `CHANGELOG.md` 的对应版本，
  分别展示完整的英文和简体中文内容，不再使用简短的提交标题；缺少翻译时会停止生成。

## 0.6.1 - 2026-09-18

### Fixed

- Updated the Android Compose host to derive manual and automatic Picture in
  Picture aspect ratios from display dimensions, decoder observations, and
  selected-track metadata, preserving portrait video instead of forcing 16:9.

## 0.6.0 - 2026-09-17

### Added

- Exposed native video display dimensions separately from per-view content
  rectangles for portrait playback, overlay positioning, and content hit tests.
- Added source/output-scoped HDR evidence snapshots and Flutter confirmed
  content-tap handling for host overlays.

### Changed

- **Breaking:** replaced Stage `isPortrait` / SwiftUI `isCompactLayout` with
  explicit `controlLayout`, independent of required `isFullscreen`, and renamed
  `landscapeControlBarLeading` to `expandedControlBarLeading` on all UI surfaces.
- Android PiP now prefers video dimensions, refreshes automatic-entry parameters,
  and consistently routes system events to the configured player.

### Fixed

- Cancelled seeks and restored temporary speed during Stage layout changes;
  prevented disposed progress bars from committing stale pointer-up events.
- Cleared detached view geometry and released iOS Flutter platform-view channels.
- Preserved platform PiP ratio bounds after rational rounding.
- Expanded Flutter button hit targets, exposed adjustable native timelines to
  accessibility, retained cleared snapshot errors, and confined Android stop
  and HDR invalidation to the main thread.

## 0.5.6 - 2026-09-16

### Fixed

- Treated unavailable Android display HDR capabilities as an empty capability
  set for compatibility with the Android 37 SDK annotations.
- Stabilized the iOS native-frame fallback regression by waiting for the
  asynchronous system-player route and diagnostic before asserting it.

### Changed

- Raised the Android compile SDK to 37 while retaining minSdk 26 and targetSdk
  36, so Android 8.0 and newer devices remain installable.
- Raised the minimum supported Flutter toolchain to 3.47.2.

## 0.5.5 - 2026-09-16

### Fixed

- Bounded playback EventHook shutdown and moved iOS disposal off the main
  thread; timed-out callbacks retain their resources until they return.
- Required verified embedded registry handles for mobile SourceNormalizer,
  decoder, and FrameProcessor loading, and reused loaded native instances.
- Preserved relay JNI failures and panics as read errors so truncated HTTP
  responses cannot be reported as normal end-of-stream.
- Delivered DLNA route callbacks outside the route lock, preserving the final
  empty snapshot across concurrent stop and callback failures.
- Rejected oversized DASH SegmentTemplate Number formatting widths before
  allocating initialization or media resource paths.

### Changed

- Updated Rust dependencies, including Clap 4.6.7, crossbeam-queue 0.3.14,
  indexmap 2.14.2, TOML 1.1.6, UUID 1.26.1, Wasmtime 48.0.2, WIT Bindgen 0.62.0,
  and WIT parser/component tooling 0.259.0, with refreshed lockfiles.
- Updated Android Media3 to 1.11.1, Lifecycle to 2.11.0, and AGP to 9.1.1.
  Consumer Kotlin tooling uses 2.4.20; published AAR metadata retains Kotlin
  2.2.10 for AGP 9.1 compatibility.
- Updated Flutter material_ui to 1.3.0 and CI to Flutter 3.47.2.
- Retained Android SDK 36, Compose BOM 2026.06.01, AndroidX Core 1.18.0, and
  OkHttp 5.4.0; newer Core, Compose UI, and OkHttp releases require SDK 37.

## 0.5.4 - 2026-09-09

### Fixed

- Restored the Kotlin 2.2.10 compiler baseline for published Android AARs so
  AGP 9.1 built-in Kotlin consumers can compile against their metadata while
  Flutter and Compose applications remain free to use Kotlin 2.4.x.
- Updated the iOS optional-release policy-drift regression fixture to exercise
  an alternate FFmpeg 9.0.x source version under the 9.0.1 compatibility lock.

## 0.5.3 - 2026-09-09

### Added

- Added Android and iOS physical-device lifecycle suites for local 720p/1080p
  playback, network HLS/DASH, Live-DVR, surface recreation, backgrounding, and
  recovery evidence.
- Added bounded Android video-frame metadata windows for frame pacing and
  missing-frame diagnostics.

### Changed

- Raised the workspace, plugin templates, release tooling, and CI minimum Rust
  version to 1.98.1; refreshed direct Rust dependencies including CPAL 0.18.2,
  wgpu 30.0.1, and syn 3.0.5.
- Upgraded the optional FFmpeg build and redistribution source lock to 9.0.1,
  and cached source archives across Android, iOS, and Flutter CI jobs.
- Aligned Android Compose tooling on Kotlin 2.4.10 and retained OkHttp 5.4.0 as
  the latest release compatible with the Android SDK 36 build boundary.
- Split the shared runtime command, media, option, event-queue, and resilience
  policy types into focused modules while preserving their public re-exports.

### Fixed

- Accepted Android native-library locators returned by the application class
  loader, including modern uncompressed `base.apk!/lib/...` entries, so optional
  plugins no longer require legacy JNI packaging in host applications.
- Kept live HLS manifests out of the media cache and added bounded
  behind-live-window recovery on Android.
- Replaced Android enum ordinals in persisted and JNI payloads with explicit
  wire values so declaration reordering cannot corrupt stored playback state.
- Redirected iOS DASH fMP4 segment requests to their HTTPS origins while
  preserving request headers, fixing physical-device playback failures from
  custom-scheme byte responses.
- Removed force-unwrapped iOS Documents-directory assumptions from download
  state and output path resolution.

## 0.5.2 - 2026-09-03

### Added

- Added the optional official performance diagnostics `BenchmarkSink`, hot
  session APIs for Flutter, Android, and iOS, bounded schema-v1 reports, and
  independently published Maven, SwiftPM, XCFramework, and Flutter artifacts.
- Added host content overlays, landscape control-row slots, and optional
  state-labelled navigation actions to the Flutter, Compose, and SwiftUI Stage
  packages.
- Added Flutter control visibility retention for focused host inputs and open
  host drawers.

### Changed

- Defined the Stage layer order so host content remains below gestures and
  playback controls, receives no input, and is removed from Picture in Picture
  presentation.

### Fixed

- Avoided `MediaCodec.setOutputSurface` on affected MediaTek OMX decoders through
  Android 10, allowing delayed or replaced video surfaces to recreate the codec
  instead of failing playback.

## 0.5.1 - 2026-09-01

### Added

- Added crates.io distribution for the Rust plugin author SDK, ABI, macros,
  package and loader layers, WASM guest and host support, bounded process
  supervision, and the `vesper` CLI.
- Added a resumable dependency-ordered plugin SDK publisher and a dedicated
  `plugin-sdk-v<version>` workflow that publishes the crates through a protected
  GitHub Environment and attaches Linux, Apple Silicon macOS, and Windows CLI
  archives with SHA-256 checksums.

### Changed

- Branded the public Cargo package identities as `vesper-player-*` while
  preserving the existing `player_*` Rust library and import names through
  explicit dependency aliases.

## 0.5.0 - 2026-08-29

### Breaking Changes

- Rebuilt the unreleased plugin runtime around typed catalog records,
  dependency resolution, invocation plans, bounded runtime scopes, and explicit
  transport/workload policy. Plugin projects must regenerate metadata and
  packages against the 0.5 SDK; compatibility aliases for the prior draft
  contract are not retained.
- Raised the workspace MSRV and release/tooling pins from Rust 1.97 to Rust
  1.98. Generated plugin templates and diagnostic fixtures use the same minimum
  version.
- Advanced first-party plugin versions to `0.5.0` and their host SDK range to
  `>=0.5.0, <0.6.0`.

### Added

- Added a Native `AudioProcessor` ABI, safe Rust author API, checked loader
  session, diagnostic plugin, host-owned timing enforcement, and distinct
  preserve-pitch and follow-rate playback processing.
- Added language-neutral plugin catalog schemas, typed provisions and
  requirements, deterministic dependency/cycle diagnostics, resolver policy,
  participation projection, cancellation, quarantine, and bounded shutdown.
- Added first-party Native AudioProcessor and WASM observer diagnostic projects
  that pass build, inspect, check, package, signature, and repeatability gates.
- Added typed plugin playback route, fallback target, and fallback reason
  diagnostics to the Flutter platform contract while preserving unknown wire
  values.

### Changed

- Upgraded the independent Android library, Compose host, wrapper tooling, and
  their CI Gradle paths to Gradle 9.7.1. Flutter CI and release validation now
  use Flutter 3.47.1 while `flutter build` remains on Gradle 9.3.1, the newest
  version accepted by Flutter 3.47.1's AGP 9.1 compatibility check.
- Tightened audio-master clock, playback-rate, generation fencing, drain, and
  A/V admission behavior across FFmpeg, CPAL, desktop runtime, and plugin
  processing paths.
- Made native-frame diagnostics report `participated` only after a frame is
  actually presented instead of when a route merely starts.

### Fixed

- Made windowed Flutter and Compose timeline scrubbing track the current pointer
  position and measured control width through forward, reverse, and
  direction-changing drags.
- Scoped Android DLNA description reuse and in-flight fetch coalescing to the
  active network binding, and stopped discovery work after a socket binding
  failure.
- Allowed the iOS subtitle device verifier to drive Flutter tests over either
  USB or local-network connections, matching its physical-device preflight.
- Extended release version updates and verification to standalone first-party
  plugin Cargo manifests and lockfiles, including generic `Unreleased`
  changelog freezing.

## 0.4.3 - 2026-08-17

### Added

- Added hosted Android SourceNormalizer and post-download remux AARs. Maven
  Central now publishes seven same-version coordinates, and each optional
  FFmpeg-backed plugin depends on the core kit plus the single shared FFmpeg
  runtime instead of bundling duplicate `libav*` libraries.
- Added `VesperPlayerSourceNormalizerFfmpeg` and
  `VesperPlayerRemuxFfmpeg` capability-level products to the remote Swift
  package. Each product closes over only its plugin and the three matching
  FFmpeg component frameworks.
- Added the `vesper_player_remux_ffmpeg` optional Flutter package and included
  both optional native dependency packages in tag-driven pub.dev publication.

### Changed

- Stable and prerelease tags now publish matching Maven, SwiftPM, and pub.dev
  versions. Prereleases retain a numeric Cargo and platform bundle version while
  using the full `-rc.N` version for public package coordinates.
- Flutter iOS optional packages resolve remote capability products and no
  longer copy a repository-local optional `Artifacts/` directory into pub
  staging.

### Fixed

- Expanded release verification to require the remux plugin, independent
  SourceNormalizer/remux/runtime/relay profile receipts, complete Maven POM
  closure, and the expected plugin registries and ELF payloads in Android sample
  APKs.
- iOS `VesperPlayerKit` release sources now mark the private
  `VesperPlayerKitBridgeShim` import as implementation-only. Release validation
  also checks Swift ABI JSON so a private shim cannot become an unresolved
  dependency for remote SwiftPM consumers.

This release candidate is intended for external consumer and physical-device
acceptance. Archive and hosted dependency checks do not by themselves establish
successful playback on a device.

## 0.4.2 - 2026-08-16

### Fixed

- Added `vesper-player-kit-external-playback` and its transitive
  `vesper-player-kit-ffmpeg-runtime` dependency to the stable Maven Central
  publication set, including same-version POM closure validation and a hosted
  Android application build after publication.
- Changed the published `vesper_player_ios` Swift package manifest to resolve
  `VesperPlayerKit` from the remote binary package, removed monorepo-local path
  discovery, and removed the nonexistent remote `VesperPlayerFFI` product
  dependency.
- Made Flutter package publication wait for the matching Maven Central
  coordinates and remote Swift package tag before acquiring Dart publication
  credentials and uploading packages.

SourceNormalizer, offline MP4 remux, and the remaining experimental mobile
plugin artifacts stay outside the default hosted dependency closure in this
release.

## 0.4.1 - 2026-08-14

### Migration

0.4 requires a coordinated host, FFI binding, and plugin upgrade. Follow the
[0.3 to 0.4 migration guide](https://github.com/umbrella22/Vesper/blob/1df8e84d64c9173018d6c047da60d46578b1ce6e/lib/MIGRATION-0.3-TO-0.4.md) before replacing any
runtime artifact.

### Breaking Changes

- Replaced the unreleased `io.github.ikaros` reverse-DNS root with
  `io.github.umbrella22` across Android packages and JNI entry points, Flutter
  plugin packages and channels, Apple bundle identifiers, and first-party
  plugin identities. No compatibility aliases are retained; update imports,
  manifest entries, custom channel names, and `VesperPluginReference` values,
  then rebuild native artifacts as one coordinated upgrade.
- Raised the Rust workspace MSRV and dedicated CI check from 1.94 to 1.97.
- Consolidated the native plugin ABI under the `vesper_plugin_entry` root and
  typed interface query contract. The loader accepts only this contract.
- Public plugin authorship is Rust Native or Rust WASM only. C/C++ author SDKs,
  mobile WASM, and plugin access to protected media are outside the plugin
  product boundary.
- Android `setSubtitleTrackSelection` is now suspending and iOS is now
  `async throws`; both complete after native confirmation.
- External subtitle source declarations now use `VesperExternalSubtitleSource`
  and `externalSubtitles`. The old type and source property remain deprecated
  aliases.
- C and iOS FFI consumers must regenerate and recompile bindings for the
  expanded `PlayerFfiError.details_json` field. C consumers must also rebuild
  for the expanded `PlayerFfiTrack` and `PlayerFfiTrackCatalog` layouts;
  replacing only the static library is not ABI-compatible.
- Removed public raw `pluginLibraryPaths` configuration from Android, iOS, and
  Flutter download and benchmark APIs. Mobile hosts now select build-time
  embedded plugins through explicit `VesperPluginReference` values.

### Added

- Added the safe Rust plugin SDK, Native and WASM scaffolds, stable interface
  identifiers, `PluginReference`, deterministic `.vesper-plugin` packaging,
  Ed25519 publisher signatures, trust-store key rotation, and install checks.
- Added the Rust `vesper plugin` CLI for scaffold, build, inspect, check,
  package, verify, install, uninstall, list, and key management workflows.
- Added the Wasmtime Component host for bounded EventHook and BenchmarkSink
  plugins with memory/fuel/deadline limits, queue caps, structured logs, and
  quarantine after traps or timeouts.
- Added a mobile subtitle baseline across Android, iOS, and Flutter: external
  SRT, WebVTT, and SSA/ASS attachment, track selection, visibility, and bounded
  font scaling. Android renders Media3 cues in the native surface host; iOS uses
  a bounded UTF-8 parser and native overlay while AVPlayer text style rules
  cover embedded subtitles.
- Added explicit RTMP, RTSP, and HTTP-FLV protocol DTO values across Rust, C,
  Android, iOS, and Flutter boundaries. Unsupported host routes fail with
  capability errors instead of falling through silently.
- Added canonical subtitle catalog/selection state, requested/confirmed/effective
  selection snapshots, structured cross-platform subtitle errors, DASH/HLS
  metadata propagation, and source-local external subtitle ids.
- Added per-track support status, bounded support diagnostics, catalog revisions,
  and playback-path identifiers across the Rust, FFI, native host, and Flutter
  contracts.
- Added expected catalog revision checks and structured fixed-track rejection
  details for explicit ABR selection.
- Added tagged iOS release artifacts for the three FFmpeg component frameworks
  and four optional plugin frameworks. The release workflow requires a generated
  FFmpeg compliance bundle and exactly one corresponding source archive before
  those XCFrameworks can be published, and binds that archive to the SHA-256
  recorded in each framework's build metadata.
- Added a physical-device optional-plugin verifier that materializes a retained,
  sanitized snapshot of the verified Release archives into an isolated Xcode
  project. It records both the original Release ZIP and tested ZIP SHA-256 values
  in `verified-release-inputs.json` and preserves the exact XCResult bundle.

### Changed

- Updated the published Android host-kit baseline to AGP 9.1-compatible Kotlin
  2.2.10 while the native Compose and Flutter consumer hosts use Kotlin
  2.4.10, plus Media3 1.11.0,
  kotlinx.coroutines 1.11.0, AndroidX Activity 1.13.0, AppCompat 1.8.0,
  Fragment 1.9.0, Lifecycle 2.10.0, Compose BOM 2026.06.01, OkHttp 5.4.0,
  and `material_ui` 1.0.0.
- Updated Flutter CI and publication workflows to Flutter 3.47.0 while keeping
  the public package compatibility floor at Flutter 3.44.0.
- Moved the repository Codex marketplace and maintainer plugins under
  `.agents/plugins`, reserving the root `plugins/` directory for Vesper runtime
  plugin projects.
- Isolated Gradle distributions, service homes, and Android build locks under
  their Android projects; Vesper CLI commands no longer create repository-root
  `.gradle/` state.
- Expanded the Stage UI integration note into `lib/README.md`, the canonical
  Android, iOS, and Flutter package guide, and corrected the iOS temporary-rate
  callback and Flutter mobile-only examples.
- HTTP URLs ending in `.flv` remain progressive VOD sources unless callers use
  the explicit `flvLive` source factory.
- Native-frame plugin breakers now enforce queue/in-flight load decisions and
  reset consecutive failure counters after every successful adapter call.
- Subtitle selection restore, source refresh, and stale callback handling now
  pass through bounded source-epoch and command-id coordination.
- Fixed-track commands now revalidate the current catalog and platform support
  before changing ABR state; iOS retains best-effort variant pinning when exact
  support cannot be established.
- Subtitle selection now resolves stable track identities and confirms the
  applied selection within one bounded readiness/readback deadline.
- Native and Flutter iOS hosts now consume seven direct products from the
  `VesperPlayerOptionalPlugins` Swift package, one for each FFmpeg component or
  plugin framework, and embed and sign them as top-level siblings.
  The flat-dylib and legacy umbrella-runtime embedding paths were removed.
- Removed product-level plugin generation labels from the public entry symbol,
  loader types, WIT package, schemas, mobile registry paths, templates, and
  diagnostics. ABI and interface major/minor fields remain the compatibility
  contract.

### Fixed

- iOS native-frame initialization now reports its pending surface state before
  asynchronous plugin diagnostics, preventing surface attachment from racing
  command readiness.
- Android instrumentation JNI staging now stays inside the host-kit module when
  Flutter redirects Gradle build directories, preserving the Rust CLI output
  boundary. The Android examples also serialize FFmpeg relay generation after
  remux and SourceNormalizer plugin tasks that share the same profile output.
- Android external-playback themes now scope `android:isLightTheme` to API 29+
  resources, and the Flutter package no longer contributes a self-referential
  route-theme alias during resource merging.
- Mobile source selection and seek commands now have bounded, generation-fenced
  native completion semantics across Android, iOS, and Flutter. Pausing during
  an in-flight source load cancels pending autoplay and preserves a paused
  snapshot even at timeline position zero. Obsolete commands fail only their
  originating call without replacing the current playback error.

- Redacted credentials, query parameters, and fragments from iOS playback
  diagnostics, including current-source details, lifecycle and retry logs, HDR
  evidence, DASH network errors, and AVPlayer error-log evidence.
- Redacted iOS foreground-download diagnostic URLs while preserving complete
  stale-resource URIs for recovery callbacks and retried requests.
- Preserved structured subtitle error details across Rust, C FFI, Android
  JNI/Kotlin, and Flutter boundaries, including malformed JSON payloads and
  unknown enum values.
- Preserved structured fixed-track rejection details, including catalog
  revisions and capability evidence, across the C FFI, Android, iOS, and
  Flutter boundaries.
- Isolated obsolete iOS subtitle selection transactions so source changes,
  disposal, and superseding commands cannot restore stale backend state,
  overwrite newer selection state, or publish stale player errors.
- Prevented obsolete Android subtitle selection failures from overwriting newer
  Flutter session errors or emitting stale player error events while preserving
  the original MethodChannel error for the superseded command.
- Core iOS framework archives now keep `VesperPlayerKitBridgeShim` internal,
  use one canonical XCFramework, omit AppleDouble metadata, and pass an isolated
  textual-interface import and link smoke before Release upload.
- Hardened optional iOS release validation to preserve whitespace in Mach-O
  dependency locators, reject undeclared dynamic dependencies and extra
  assets/slices, verify FFmpeg compliance file contents against their sources,
  force FFmpeg source rebuilds, cross-check device and Simulator metadata, and
  remove retired GitHub Release assets during same-tag reruns.
- Aligned Android CI and release jobs with the Gradle 9.6.0 wrapper, corrected
  the Flutter Android minimum requirement to 3.44.0, and synchronized Android
  source-build toolchain documentation.
- Preserved usable subtitle catalogs after partial external resource failures
  while reporting all-track failures and duplicate identity/default conflicts.
- Fixed iOS local audio-only playback waiting for a video frame and separated
  explicit automatic subtitle selection from the startup subtitle-enable gate.

## 0.3.1 - 2026-06-09

### Breaking Changes

- Rust: `player-runtime` no longer exports the unused lightweight async controller
  types (`Player`, `PlayerHandle`, `PlayerConfig`, `PlaybackCommand`, and
  `PlayerEvent`) and no longer depends on Tokio.

## 0.3.0 - 2026-05-18

### Breaking Changes

- FFmpeg mobile builds now use `./scripts/vesper ffmpeg --platform android|ios|all --profile <name>` as the public CLI. The old public `android ffmpeg`, `android ffmpeg-runtime`, and `apple ffmpeg` commands were removed from `scripts/vesper`.
- Android Cast, DLNA, relay, and relay FFmpeg split modules were consolidated into `vesper-player-kit-external-playback` with public APIs under `io.github.ikaros.vesper.player.android.external`.

### Added

- Added `scripts/ffmpeg-profiles.toml` with `base`, `download-remux`, `relay-remux`, and `default` FFmpeg profiles, including inheritance, platform overrides, overlays, validation, and stable profile hashes.
- Added Android release staging for `VesperPlayerKitComposeUi`, `VesperPlayerKitExternalPlayback`, and `VesperPlayerKitFfmpegRuntime` AARs.
- Added optional iOS `VesperPlayerFfmpegRuntime.xcframework.zip` and `VesperPlayerRemuxFfmpegPlugin.xcframework.zip` staging so FFmpeg-backed remux support stays out of the core `VesperPlayerKit.xcframework`.

### Changed

- `download-remux`, `relay-remux`, and `default` profiles validate local-only remux builds with network and OpenSSL disabled by default.
- Flutter external playback on Android now calls the consolidated Kotlin external playback facade while preserving the Dart API.

## 0.2.0 - 2026-05-13

### Breaking Changes

- Android: JNI, bridge, and `Native*` payload types are no longer public API. Use `VesperPlayerController`, `VesperPlayerSource`, `VesperTrackSelection`, `VesperVideoSurfaceKind`, and the download/preload facades instead.
- Android: DLNA and relay artifacts no longer set global `android:usesCleartextTraffic="true"`. Host apps that use local-network HTTP playback must opt in explicitly in their own manifest or network security configuration.
- Android: `vesper-player-kit-compose` now provides only controller binding and surface attachment. Visual styling such as rounded corners, background, and outline belongs in `vesper-player-kit-compose-ui` or host UI.
- Flutter: external playback DTOs moved to `vesper_player_platform_interface`; `vesper_player_external_playback` no longer owns parallel public DTO definitions.
- Rust: `player-model` is now a DTO-only crate. The Tokio actor/controller types moved to `player-runtime`.
- Rust: `DownloadTaskSnapshot.asset_index` is now shared as `Arc<DownloadAssetIndex>` so polling manager snapshots no longer deep-clones resource and segment lists.
- Rust/plugin ABI: decoder plugins now use ABI v3 and typed native device context payloads such as `DecoderNativeDeviceContext::D3D11Device { device_ptr }`. The old generic `{ kind, handle }` native context payload is not accepted.
- iOS FFI: `player-ffi-ios` error codes and categories now align with the desktop FFI error taxonomy.

### Fixed

- Added panic containment to iOS FFI entry points, macOS AVFoundation callbacks, and plugin-loader progress callbacks.
- Added panic containment to decoder and remux plugin entry points and ABI callbacks so plugin panics are mapped to ABI error payloads.
- Replaced production panic paths in Windows probing, timeline ratio seeking, FFmpeg timestamp/audio conversion, and JNI signature helpers.
- Reworked `player-audio-cpal` around an `rtrb` SPSC audio ring so the CPAL callback no longer locks the shared timeline or allocates output chunks.
- Added shared iOS `AVAudioSession` ownership, interruption handling, route-change pause behavior, and `isInterrupted` state propagation.
- Restricted Android relay default binding to a Wi-Fi LAN address and fail fast when no LAN address is available.
- Hardened Android DLNA discovery so stale description fetches cannot publish routes after discovery stops, and SSDP NOTIFY falls back when port 1900 is already bound.
- Clamped download progress updates to known byte and segment totals, and forced paused in-flight preparation tasks to re-run preparation before resuming.
- Added a backend-level FFmpeg `MasterClock` abstraction with audio-as-master selection for desktop A/V synchronization.
- Added Android host-kit API surface verification to block bridge, JNI, and `Native*` implementation types from re-entering the public API.
- Preserved Flutter viewport state across app lifecycle transitions.
