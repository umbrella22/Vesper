#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#import <QuartzCore/QuartzCore.h>
#import <dispatch/dispatch.h>
#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

typedef void (*BasicPlayerMacosOverlayActionCallback)(void *context,
                                                      uint32_t action_kind,
                                                      float playback_rate);

typedef struct {
    BasicPlayerMacosOverlayActionCallback on_action;
    void *context;
} BasicPlayerMacosOverlayCallbacks;

typedef struct {
    uint8_t is_playing;
    uint8_t has_duration;
    uint32_t timeline_kind;
    uint8_t is_seekable;
    uint8_t controls_visible;
    uint64_t position_ms;
    uint64_t duration_ms;
    uint64_t seekable_start_ms;
    uint64_t seekable_end_ms;
    float playback_rate;
} BasicPlayerMacosOverlayState;

typedef NS_ENUM(uint32_t, BasicPlayerMacosOverlayActionKind) {
    BasicPlayerMacosOverlayActionKindSeekStart = 0,
    BasicPlayerMacosOverlayActionKindSeekBack = 1,
    BasicPlayerMacosOverlayActionKindTogglePause = 2,
    BasicPlayerMacosOverlayActionKindStop = 3,
    BasicPlayerMacosOverlayActionKindSeekForward = 4,
    BasicPlayerMacosOverlayActionKindSeekEnd = 5,
    BasicPlayerMacosOverlayActionKindSetRate = 6,
    BasicPlayerMacosOverlayActionKindSeekToRatio = 7,
};

static void player_copy_utf8(const char *source, char *target, size_t target_size) {
    if (target_size == 0) {
        return;
    }

    if (source == NULL) {
        target[0] = '\0';
        return;
    }

    strncpy(target, source, target_size - 1);
    target[target_size - 1] = '\0';
}

static void player_copy_nsstring(NSString *source, char *target, size_t target_size) {
    if (source == nil) {
        player_copy_utf8(NULL, target, target_size);
        return;
    }

    player_copy_utf8(source.UTF8String, target, target_size);
}

static void player_write_error_message(NSString *message, char *target, size_t target_size) {
    player_copy_nsstring(message, target, target_size);
}

static void player_run_sync_on_main(dispatch_block_t block) {
    if ([NSThread isMainThread]) {
        block();
    } else {
        dispatch_sync(dispatch_get_main_queue(), block);
    }
}

static NSString *player_format_duration(uint64_t millis) {
    uint64_t total_seconds = millis / 1000;
    uint64_t minutes = total_seconds / 60;
    uint64_t seconds = total_seconds % 60;
    return [NSString stringWithFormat:@"%02llu:%02llu", minutes, seconds];
}

@class BasicPlayerMacosOverlayController;

@interface BasicPlayerMacosOverlayButton : NSButton
@end

@interface BasicPlayerMacosOverlayView : NSView
@property(nonatomic, weak) BasicPlayerMacosOverlayController *owner;
@end

@implementation BasicPlayerMacosOverlayButton

- (NSView *)hitTest:(NSPoint)point {
    return NSPointInRect(point, self.bounds) ? self : nil;
}

- (BOOL)acceptsFirstMouse:(NSEvent *)event {
    (void)event;
    return YES;
}

- (BOOL)acceptsFirstResponder {
    return YES;
}

- (BOOL)mouseDownCanMoveWindow {
    return NO;
}

- (void)mouseUp:(NSEvent *)event {
    NSPoint point = [self convertPoint:event.locationInWindow fromView:nil];
    if (!NSPointInRect(point, self.bounds)) {
        return;
    }

    if (self.target != nil && self.action != NULL) {
        [NSApp sendAction:self.action to:self.target from:self];
    }
}

@end

@interface BasicPlayerMacosOverlayController : NSObject

@property(nonatomic, weak) NSView *hostView;
@property(nonatomic, weak) NSView *containerView;
@property(nonatomic, strong) BasicPlayerMacosOverlayView *overlayView;
@property(nonatomic, strong) id localMouseMonitor;
@property(nonatomic, strong) CALayer *progressTrackLayer;
@property(nonatomic, strong) CALayer *progressFillLayer;
@property(nonatomic, strong) NSButton *seekStartButton;
@property(nonatomic, strong) NSButton *seekBackButton;
@property(nonatomic, strong) NSButton *playPauseButton;
@property(nonatomic, strong) NSButton *stopButton;
@property(nonatomic, strong) NSButton *seekForwardButton;
@property(nonatomic, strong) NSButton *seekEndButton;
@property(nonatomic, strong) NSArray<NSButton *> *rateButtons;
@property(nonatomic, strong) NSTextField *timeLabel;
@property(nonatomic, assign) BasicPlayerMacosOverlayCallbacks callbacks;
@property(nonatomic, assign) BasicPlayerMacosOverlayState currentState;
@property(nonatomic, assign) BOOL draggingProgress;
@property(nonatomic, assign) CGFloat dragProgressRatio;

- (instancetype)initWithHostView:(NSView *)hostView
                       callbacks:(BasicPlayerMacosOverlayCallbacks)callbacks
                           error:(NSString **)error;
- (void)updateState:(BasicPlayerMacosOverlayState)state;
- (void)layoutControls;
- (void)refreshVisualState;
- (CGFloat)overlayOriginYForHostFrame:(NSRect)host_frame
                            barHeight:(CGFloat)bar_height
                    containerFlipped:(BOOL)container_flipped;
- (BOOL)handleLocalMouseEvent:(NSEvent *)event;
- (BOOL)isScrubbableTimeline;
- (BOOL)isPointInProgressTrack:(NSPoint)point;
- (CGFloat)progressRatioForPoint:(NSPoint)point;
- (CGFloat)displayedProgressRatio;
- (uint64_t)displayedPositionMilliseconds;
- (BOOL)dispatchOverlayActionAtPoint:(NSPoint)point;

@end

@implementation BasicPlayerMacosOverlayView

- (BOOL)isOpaque {
    return NO;
}

- (void)layout {
    [super layout];
    [self.owner layoutControls];
}

@end

@implementation BasicPlayerMacosOverlayController

- (instancetype)initWithHostView:(NSView *)hostView
                       callbacks:(BasicPlayerMacosOverlayCallbacks)callbacks
                           error:(NSString **)error {
    self = [super init];
    if (self == nil) {
        return nil;
    }

    if (hostView == nil) {
        if (error != NULL) {
            *error = @"received a null NSView handle for basic-player macOS host overlay";
        }
        return nil;
    }

    self.hostView = hostView;
    self.containerView = hostView.window.contentView ?: hostView.superview ?: hostView;
    self.callbacks = callbacks;

    BasicPlayerMacosOverlayView *overlayView =
        [[BasicPlayerMacosOverlayView alloc] initWithFrame:NSZeroRect];
    overlayView.owner = self;
    overlayView.wantsLayer = YES;
    overlayView.layer.cornerRadius = 0.0;
    overlayView.layer.masksToBounds = NO;
    overlayView.layer.backgroundColor =
        [[NSColor colorWithCalibratedRed:10.0 / 255.0
                                   green:14.0 / 255.0
                                    blue:18.0 / 255.0
                                   alpha:178.0 / 255.0] CGColor];
    overlayView.autoresizingMask = NSViewNotSizable;
    self.overlayView = overlayView;

    self.progressTrackLayer = [CALayer layer];
    self.progressTrackLayer.backgroundColor =
        [[NSColor colorWithCalibratedWhite:1.0 alpha:38.0 / 255.0] CGColor];
    [overlayView.layer addSublayer:self.progressTrackLayer];

    self.progressFillLayer = [CALayer layer];
    self.progressFillLayer.backgroundColor =
        [[NSColor colorWithCalibratedRed:244.0 / 255.0
                                   green:184.0 / 255.0
                                    blue:96.0 / 255.0
                                   alpha:1.0] CGColor];
    [overlayView.layer addSublayer:self.progressFillLayer];

    self.seekStartButton = [self makeButtonWithTitle:@"|<"
                                              action:@selector(onSeekStart:)];
    self.seekBackButton = [self makeButtonWithTitle:@"<<"
                                             action:@selector(onSeekBack:)];
    self.playPauseButton = [self makeButtonWithTitle:@">"
                                              action:@selector(onTogglePause:)];
    self.stopButton = [self makeButtonWithTitle:@"[]"
                                         action:@selector(onStop:)];
    self.seekForwardButton = [self makeButtonWithTitle:@">>"
                                                action:@selector(onSeekForward:)];
    self.seekEndButton = [self makeButtonWithTitle:@">|"
                                            action:@selector(onSeekEnd:)];

    NSMutableArray<NSButton *> *rateButtons = [NSMutableArray array];
    NSArray<NSString *> *rateTitles = @[ @"0.5X", @"1X", @"1.5X", @"2X", @"3X" ];
    for (NSUInteger index = 0; index < rateTitles.count; index += 1) {
        NSButton *button = [self makeButtonWithTitle:rateTitles[index]
                                              action:@selector(onRateChanged:)];
        button.tag = (NSInteger)index;
        [rateButtons addObject:button];
    }
    self.rateButtons = rateButtons;

    self.timeLabel = [self makeLabelWithFontSize:13.0 weight:NSFontWeightSemibold];
    self.timeLabel.alignment = NSTextAlignmentCenter;
    self.timeLabel.stringValue = @"00:00/--:--";

    for (NSView *subview in @[
             self.seekStartButton,
             self.seekBackButton,
             self.playPauseButton,
             self.stopButton,
             self.seekForwardButton,
             self.seekEndButton,
             self.timeLabel,
         ]) {
        [overlayView addSubview:subview];
    }
    for (NSButton *button in self.rateButtons) {
        [overlayView addSubview:button];
    }

    NSView *container_view = self.containerView ?: hostView;
    [container_view addSubview:overlayView positioned:NSWindowAbove relativeTo:nil];
    __weak typeof(self) weak_self = self;
    self.localMouseMonitor = [NSEvent
        addLocalMonitorForEventsMatchingMask:(NSEventMaskLeftMouseDown |
                                             NSEventMaskLeftMouseDragged |
                                             NSEventMaskLeftMouseUp)
                                     handler:^NSEvent *_Nullable(NSEvent *event) {
                                       __strong typeof(weak_self) strong_self = weak_self;
                                       if (strong_self == nil) {
                                           return event;
                                       }

                                       return [strong_self handleLocalMouseEvent:event] ? nil : event;
                                     }];
    self.currentState = (BasicPlayerMacosOverlayState){
        .is_playing = 0,
        .has_duration = 0,
        .timeline_kind = 0,
        .is_seekable = 0,
        .controls_visible = 1,
        .position_ms = 0,
        .duration_ms = 0,
        .seekable_start_ms = 0,
        .seekable_end_ms = 0,
        .playback_rate = 1.0f,
    };
    [self refreshVisualState];
    [self layoutControls];
    return self;
}

- (void)dealloc {
    if (self.localMouseMonitor != nil) {
        [NSEvent removeMonitor:self.localMouseMonitor];
        self.localMouseMonitor = nil;
    }
    [self.overlayView removeFromSuperview];
}

- (NSButton *)makeButtonWithTitle:(NSString *)title action:(SEL)action {
    BasicPlayerMacosOverlayButton *button =
        [[BasicPlayerMacosOverlayButton alloc] initWithFrame:NSZeroRect];
    button.title = title;
    button.target = self;
    button.action = action;
    button.bordered = NO;
    button.wantsLayer = YES;
    button.layer.cornerRadius = 8.0;
    button.layer.masksToBounds = YES;
    button.layer.borderWidth = 2.0;
    button.font = [NSFont monospacedSystemFontOfSize:13.0 weight:NSFontWeightBold];
    return button;
}

- (NSTextField *)makeLabelWithFontSize:(CGFloat)font_size weight:(NSFontWeight)weight {
    NSTextField *label = [[NSTextField alloc] initWithFrame:NSZeroRect];
    label.editable = NO;
    label.bezeled = NO;
    label.drawsBackground = NO;
    label.selectable = NO;
    label.textColor = [NSColor colorWithCalibratedWhite:0.96 alpha:1.0];
    label.font = [NSFont systemFontOfSize:font_size weight:weight];
    return label;
}

- (void)layoutControls {
    if (self.hostView == nil || self.overlayView == nil) {
        return;
    }

    NSView *container_view = self.containerView ?: self.hostView;
    NSRect host_frame =
        [self.hostView convertRect:self.hostView.bounds toView:container_view];
    CGFloat bar_height = MIN(MAX(host_frame.size.height / 5.0, 60.0), 88.0);
    self.overlayView.frame = NSMakeRect(
        host_frame.origin.x,
        [self overlayOriginYForHostFrame:host_frame
                               barHeight:bar_height
                       containerFlipped:[container_view isFlipped]],
        host_frame.size.width,
        bar_height);

    self.progressTrackLayer.frame =
        CGRectMake(0.0, bar_height - 4.0, self.overlayView.bounds.size.width, 4.0);
    CGFloat progress_ratio = [self displayedProgressRatio];
    self.progressFillLayer.frame =
        CGRectMake(0.0, bar_height - 4.0, floor(self.overlayView.bounds.size.width * progress_ratio), 4.0);

    CGFloat padding = MAX(floor(bar_height / 5.0), 8.0);
    CGFloat gap = MAX(floor(padding / 2.0), 8.0);
    CGFloat icon_size = bar_height - padding * 2.0;
    CGFloat rate_width = MAX(icon_size + 20.0, 58.0);
    CGFloat button_y = padding;
    CGFloat x = padding;

    NSArray<NSButton *> *buttons = @[
        self.seekStartButton,
        self.seekBackButton,
        self.playPauseButton,
        self.stopButton,
        self.seekForwardButton,
        self.seekEndButton,
    ];

    for (NSUInteger index = 0; index < buttons.count; index += 1) {
        buttons[index].frame = NSMakeRect(x, button_y, icon_size, icon_size);
        x += icon_size + gap;
    }

    CGFloat total_rate_width =
        self.rateButtons.count * rate_width + MAX((CGFloat)self.rateButtons.count - 1.0, 0.0) * gap;
    CGFloat rate_x = self.overlayView.bounds.size.width - padding - total_rate_width;
    for (NSUInteger index = 0; index < self.rateButtons.count; index += 1) {
        NSButton *button = self.rateButtons[index];
        button.frame = NSMakeRect(rate_x, button_y, rate_width, icon_size);
        rate_x += rate_width + gap;
    }

    self.timeLabel.frame = NSMakeRect(0.0,
                                      button_y + floor((icon_size - 14.0) * 0.5),
                                      self.overlayView.bounds.size.width,
                                      14.0);
}

- (CGFloat)overlayOriginYForHostFrame:(NSRect)host_frame
                            barHeight:(CGFloat)bar_height
                    containerFlipped:(BOOL)container_flipped {
    if (container_flipped) {
        return MAX(NSMaxY(host_frame) - bar_height, 0.0);
    }

    return NSMinY(host_frame);
}

- (void)refreshVisualState {
    self.overlayView.hidden = (self.currentState.controls_visible == 0);
    [self applyButtonStyle:self.seekStartButton active:NO];
    [self applyButtonStyle:self.seekBackButton active:NO];
    [self applyButtonStyle:self.stopButton active:NO];
    [self applyButtonStyle:self.seekForwardButton active:NO];
    [self applyButtonStyle:self.seekEndButton active:NO];
    [self applyButtonStyle:self.playPauseButton active:YES];
    self.playPauseButton.title = self.currentState.is_playing != 0 ? @"||" : @"|>";

    NSArray<NSNumber *> *rates = @[ @0.5f, @1.0f, @1.5f, @2.0f, @3.0f ];
    for (NSUInteger index = 0; index < self.rateButtons.count; index += 1) {
        float rate = rates[index].floatValue;
        BOOL active = fabsf(self.currentState.playback_rate - rate) < 0.05f;
        [self applyButtonStyle:self.rateButtons[index] active:active];
    }

    uint64_t displayed_position_ms = [self displayedPositionMilliseconds];
    NSString *time_label =
        self.currentState.has_duration != 0
            ? [NSString stringWithFormat:@"%@/%@",
                                       player_format_duration(displayed_position_ms),
                                       player_format_duration(self.currentState.duration_ms)]
            : player_format_duration(displayed_position_ms);
    self.timeLabel.stringValue = time_label;
    [self layoutControls];
}

- (BOOL)handleLocalMouseEvent:(NSEvent *)event {
    if (self.overlayView.hidden || event.window != self.hostView.window) {
        return NO;
    }

    NSPoint point = [self.overlayView convertPoint:event.locationInWindow fromView:nil];
    switch (event.type) {
        case NSEventTypeLeftMouseDown:
            if ([self isScrubbableTimeline] && [self isPointInProgressTrack:point]) {
                self.draggingProgress = YES;
                self.dragProgressRatio = [self progressRatioForPoint:point];
                [self refreshVisualState];
                return YES;
            }
            return NO;
        case NSEventTypeLeftMouseDragged:
            if (!self.draggingProgress) {
                return NO;
            }
            self.dragProgressRatio = [self progressRatioForPoint:point];
            [self refreshVisualState];
            return YES;
        case NSEventTypeLeftMouseUp:
            if (self.draggingProgress) {
                self.dragProgressRatio = [self progressRatioForPoint:point];
                BasicPlayerMacosOverlayState state = self.currentState;
                state.position_ms = [self displayedPositionMilliseconds];
                self.currentState = state;
                self.draggingProgress = NO;
                [self emitActionKind:BasicPlayerMacosOverlayActionKindSeekToRatio
                                rate:(float)self.dragProgressRatio];
                [self refreshVisualState];
                return YES;
            }
            return [self dispatchOverlayActionAtPoint:point];
        default:
            return NO;
    }
}

- (BOOL)isScrubbableTimeline {
    if (self.currentState.is_seekable == 0) {
        return NO;
    }

    return self.currentState.timeline_kind == 0 || self.currentState.timeline_kind == 2;
}

- (BOOL)isPointInProgressTrack:(NSPoint)point {
    CGRect hit_rect = self.progressTrackLayer.frame;
    hit_rect.origin.y = MAX(hit_rect.origin.y - 8.0, 0.0);
    hit_rect.size.height += 12.0;
    return NSPointInRect(point, hit_rect);
}

- (CGFloat)progressRatioForPoint:(NSPoint)point {
    CGFloat width = MAX(self.progressTrackLayer.frame.size.width, 1.0);
    return MIN(MAX(point.x / width, 0.0), 1.0);
}

- (CGFloat)displayedProgressRatio {
    if (self.draggingProgress) {
        return MIN(MAX(self.dragProgressRatio, 0.0), 1.0);
    }

    if (self.currentState.is_seekable != 0 &&
        self.currentState.seekable_end_ms > self.currentState.seekable_start_ms) {
        uint64_t clamped_position = MIN(MAX(self.currentState.position_ms,
                                            self.currentState.seekable_start_ms),
                                        self.currentState.seekable_end_ms);
        uint64_t offset = clamped_position - self.currentState.seekable_start_ms;
        uint64_t total = self.currentState.seekable_end_ms - self.currentState.seekable_start_ms;
        return total == 0 ? 1.0 : (CGFloat)offset / (CGFloat)total;
    }

    if (self.currentState.has_duration != 0 && self.currentState.duration_ms > 0) {
        return MIN(MAX((CGFloat)self.currentState.position_ms / (CGFloat)self.currentState.duration_ms,
                       0.0),
                   1.0);
    }

    return 0.0;
}

- (uint64_t)displayedPositionMilliseconds {
    if (!self.draggingProgress) {
        return self.currentState.position_ms;
    }

    if (self.currentState.is_seekable != 0 &&
        self.currentState.seekable_end_ms >= self.currentState.seekable_start_ms) {
        uint64_t total = self.currentState.seekable_end_ms - self.currentState.seekable_start_ms;
        uint64_t offset = (uint64_t)llround((double)total * self.dragProgressRatio);
        return MIN(self.currentState.seekable_start_ms + offset, self.currentState.seekable_end_ms);
    }

    if (self.currentState.has_duration != 0) {
        return (uint64_t)llround((double)self.currentState.duration_ms * self.dragProgressRatio);
    }

    return self.currentState.position_ms;
}

- (BOOL)dispatchOverlayActionAtPoint:(NSPoint)point {
    if (!NSPointInRect(point, self.overlayView.bounds)) {
        return NO;
    }

    struct OverlayHitTarget {
        __unsafe_unretained NSView *view;
        BasicPlayerMacosOverlayActionKind action;
        float rate;
    };

    struct OverlayHitTarget hit_targets[] = {
        { self.seekStartButton, BasicPlayerMacosOverlayActionKindSeekStart, 0.0f },
        { self.seekBackButton, BasicPlayerMacosOverlayActionKindSeekBack, 0.0f },
        { self.playPauseButton, BasicPlayerMacosOverlayActionKindTogglePause, 0.0f },
        { self.stopButton, BasicPlayerMacosOverlayActionKindStop, 0.0f },
        { self.seekForwardButton, BasicPlayerMacosOverlayActionKindSeekForward, 0.0f },
        { self.seekEndButton, BasicPlayerMacosOverlayActionKindSeekEnd, 0.0f },
    };

    for (NSUInteger index = 0; index < sizeof(hit_targets) / sizeof(hit_targets[0]); index += 1) {
        if (!NSPointInRect(point, hit_targets[index].view.frame)) {
            continue;
        }

        [self emitActionKind:hit_targets[index].action rate:hit_targets[index].rate];
        return YES;
    }

    NSArray<NSNumber *> *rates = @[ @0.5f, @1.0f, @1.5f, @2.0f, @3.0f ];
    for (NSUInteger index = 0; index < self.rateButtons.count; index += 1) {
        NSButton *button = self.rateButtons[index];
        if (!NSPointInRect(point, button.frame)) {
            continue;
        }

        float rate = index < rates.count ? rates[index].floatValue : 1.0f;
        [self emitActionKind:BasicPlayerMacosOverlayActionKindSetRate rate:rate];
        return YES;
    }

    return NO;
}

- (void)applyButtonStyle:(NSButton *)button active:(BOOL)active {
    NSColor *fill_color = active
                              ? [NSColor colorWithCalibratedRed:244.0 / 255.0
                                                           green:184.0 / 255.0
                                                            blue:96.0 / 255.0
                                                           alpha:238.0 / 255.0]
                              : [NSColor colorWithCalibratedWhite:1.0 alpha:30.0 / 255.0];
    NSColor *border_color = active
                                ? [NSColor colorWithCalibratedRed:1.0
                                                             green:246.0 / 255.0
                                                              blue:218.0 / 255.0
                                                             alpha:1.0]
                                : [NSColor colorWithCalibratedWhite:1.0 alpha:80.0 / 255.0];
    NSColor *text_color = active
                              ? [NSColor colorWithCalibratedRed:28.0 / 255.0
                                                           green:24.0 / 255.0
                                                            blue:20.0 / 255.0
                                                           alpha:1.0]
                              : [NSColor colorWithCalibratedWhite:244.0 / 255.0 alpha:1.0];

    button.layer.backgroundColor = fill_color.CGColor;
    button.layer.borderColor = border_color.CGColor;
    NSDictionary<NSAttributedStringKey, id> *attributes = @{
        NSForegroundColorAttributeName : text_color,
        NSFontAttributeName :
            [NSFont monospacedSystemFontOfSize:[button.title containsString:@"X"] ? 12.0 : 13.0
                                        weight:NSFontWeightBold]
    };
    button.attributedTitle =
        [[NSAttributedString alloc] initWithString:button.title attributes:attributes];
}

- (void)emitActionKind:(BasicPlayerMacosOverlayActionKind)action_kind rate:(float)rate {
    if (self.callbacks.context == NULL || self.callbacks.on_action == NULL) {
        return;
    }

    self.callbacks.on_action(self.callbacks.context, action_kind, rate);
}

- (void)updateState:(BasicPlayerMacosOverlayState)state {
    if (state.playback_rate <= 0.0f) {
        state.playback_rate = 1.0f;
    }
    self.currentState = state;
    [self refreshVisualState];
}

- (void)onSeekStart:(id)sender {
    (void)sender;
    [self emitActionKind:BasicPlayerMacosOverlayActionKindSeekStart rate:0.0f];
}

- (void)onSeekBack:(id)sender {
    (void)sender;
    [self emitActionKind:BasicPlayerMacosOverlayActionKindSeekBack rate:0.0f];
}

- (void)onTogglePause:(id)sender {
    (void)sender;
    [self emitActionKind:BasicPlayerMacosOverlayActionKindTogglePause rate:0.0f];
}

- (void)onStop:(id)sender {
    (void)sender;
    [self emitActionKind:BasicPlayerMacosOverlayActionKindStop rate:0.0f];
}

- (void)onSeekForward:(id)sender {
    (void)sender;
    [self emitActionKind:BasicPlayerMacosOverlayActionKindSeekForward rate:0.0f];
}

- (void)onSeekEnd:(id)sender {
    (void)sender;
    [self emitActionKind:BasicPlayerMacosOverlayActionKindSeekEnd rate:0.0f];
}

- (void)onRateChanged:(id)sender {
    NSButton *button = (NSButton *)sender;
    NSArray<NSNumber *> *rates = @[ @0.5f, @1.0f, @1.5f, @2.0f, @3.0f ];
    NSInteger index = button.tag;
    float rate = (index >= 0 && index < (NSInteger)rates.count) ? rates[index].floatValue : 1.0f;
    [self emitActionKind:BasicPlayerMacosOverlayActionKindSetRate rate:rate];
}

@end

bool basic_player_macos_overlay_create(uintptr_t ns_view_handle,
                                       BasicPlayerMacosOverlayCallbacks callbacks,
                                       void **out_overlay,
                                       char *error_message,
                                       size_t error_message_size) {
    if (out_overlay == NULL) {
        player_copy_utf8("overlay output handle must not be null", error_message, error_message_size);
        return false;
    }

    @autoreleasepool {
        __block BasicPlayerMacosOverlayController *overlay = nil;
        __block NSString *create_error = nil;
        player_run_sync_on_main(^{
          NSView *host_view = (__bridge NSView *)((void *)ns_view_handle);
          overlay = [[BasicPlayerMacosOverlayController alloc] initWithHostView:host_view
                                                                      callbacks:callbacks
                                                                          error:&create_error];
        });

        if (overlay == nil) {
            player_write_error_message(create_error, error_message, error_message_size);
            return false;
        }

        *out_overlay = (__bridge_retained void *)overlay;
        player_copy_utf8(NULL, error_message, error_message_size);
        return true;
    }
}

void basic_player_macos_overlay_destroy(void *overlay_handle) {
    if (overlay_handle == NULL) {
        return;
    }

    @autoreleasepool {
        player_run_sync_on_main(^{
          BasicPlayerMacosOverlayController *overlay =
              (__bridge_transfer BasicPlayerMacosOverlayController *)overlay_handle;
          (void)overlay;
        });
    }
}

void basic_player_macos_overlay_update(void *overlay_handle, BasicPlayerMacosOverlayState state) {
    if (overlay_handle == NULL) {
        return;
    }

    @autoreleasepool {
        player_run_sync_on_main(^{
          BasicPlayerMacosOverlayController *overlay =
              (__bridge BasicPlayerMacosOverlayController *)overlay_handle;
          [overlay updateState:state];
        });
    }
}
