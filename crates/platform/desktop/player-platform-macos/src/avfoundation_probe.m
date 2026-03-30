#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <AudioToolbox/AudioToolbox.h>
#import <CoreMedia/CoreMedia.h>
#import <Foundation/Foundation.h>
#import <QuartzCore/QuartzCore.h>
#import <dispatch/dispatch.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>
#include <math.h>

typedef struct {
    uint8_t present;
    char codec[32];
    uint32_t width;
    uint32_t height;
    double frame_rate;
} PlayerMacosVideoProbe;

typedef struct {
    uint8_t present;
    char codec[32];
    uint32_t sample_rate;
    uint16_t channels;
} PlayerMacosAudioProbe;

typedef struct {
    uint8_t success;
    uint8_t has_duration;
    uint64_t duration_ms;
    uint8_t has_bit_rate;
    uint64_t bit_rate;
    uint32_t audio_streams;
    uint32_t video_streams;
    PlayerMacosVideoProbe video;
    PlayerMacosAudioProbe audio;
    char error_message[256];
} PlayerMacosAvFoundationProbeResult;

static void player_zero_probe_result(PlayerMacosAvFoundationProbeResult *result) {
    memset(result, 0, sizeof(*result));
}

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
        target[0] = '\0';
        return;
    }

    player_copy_utf8(source.UTF8String, target, target_size);
}

static NSString *player_fourcc_string(uint32_t value) {
    char characters[5] = {
        (char)((value >> 24) & 0xFF),
        (char)((value >> 16) & 0xFF),
        (char)((value >> 8) & 0xFF),
        (char)(value & 0xFF),
        '\0',
    };
    return [NSString stringWithUTF8String:characters];
}

static NSString *player_video_codec_name(uint32_t codec) {
    switch (codec) {
        case 'avc1':
        case 'avc3':
            return @"H264";
        case 'hvc1':
        case 'hev1':
            return @"HEVC";
        default:
            return [player_fourcc_string(codec) uppercaseString];
    }
}

static NSString *player_audio_codec_name(uint32_t format_id) {
    switch (format_id) {
        case kAudioFormatMPEG4AAC:
            return @"AAC";
        case kAudioFormatLinearPCM:
            return @"PCM";
        default:
            return [player_fourcc_string(format_id) uppercaseString];
    }
}

static NSURL *player_source_url_from_utf8(const char *source) {
    if (source == NULL || source[0] == '\0') {
        return nil;
    }

    NSString *input = [NSString stringWithUTF8String:source];
    if (input == nil || input.length == 0) {
        return nil;
    }

    NSURL *url = [NSURL URLWithString:input];
    if (url != nil && url.scheme.length > 0) {
        return url;
    }

    return [NSURL fileURLWithPath:input];
}

static NSError *player_load_asset(AVURLAsset *asset, NSArray<NSString *> *keys) {
    __block NSError *load_error = nil;
    dispatch_semaphore_t semaphore = dispatch_semaphore_create(0);

    [asset loadValuesAsynchronouslyForKeys:keys
                         completionHandler:^{
                           for (NSString *key in keys) {
                               NSError *key_error = nil;
                               AVKeyValueStatus status = [asset statusOfValueForKey:key
                                                                               error:&key_error];
                               if (status != AVKeyValueStatusLoaded) {
                                   load_error = key_error;
                                   if (load_error == nil) {
                                       load_error = [NSError errorWithDomain:@"player.macos.avfoundation"
                                                                         code:-1
                                                                     userInfo:@{
                                                                         NSLocalizedDescriptionKey :
                                                                             [NSString stringWithFormat:@"failed to load asset key %@", key]
                                                                     }];
                                   }
                                   break;
                               }
                           }

                           dispatch_semaphore_signal(semaphore);
                         }];

    dispatch_semaphore_wait(semaphore, DISPATCH_TIME_FOREVER);
    return load_error;
}

bool player_macos_avfoundation_probe(const char *source,
                                     PlayerMacosAvFoundationProbeResult *out_result) {
    if (out_result == NULL) {
        return false;
    }

    @autoreleasepool {
        player_zero_probe_result(out_result);

        NSURL *url = player_source_url_from_utf8(source);
        if (url == nil) {
            player_copy_utf8("failed to create NSURL from media source",
                             out_result->error_message,
                             sizeof(out_result->error_message));
            return false;
        }

        AVURLAsset *asset =
            [AVURLAsset URLAssetWithURL:url
                                options:@{
                                    AVURLAssetPreferPreciseDurationAndTimingKey : @YES,
                                }];
        NSArray<NSString *> *keys = @[ @"duration", @"tracks" ];
        NSError *load_error = player_load_asset(asset, keys);
        if (load_error != nil) {
            player_copy_nsstring(load_error.localizedDescription ?: @"failed to load AVAsset",
                                 out_result->error_message,
                                 sizeof(out_result->error_message));
            return false;
        }

        NSArray<AVAssetTrack *> *video_tracks = [asset tracksWithMediaType:AVMediaTypeVideo];
        NSArray<AVAssetTrack *> *audio_tracks = [asset tracksWithMediaType:AVMediaTypeAudio];
        out_result->video_streams = (uint32_t)video_tracks.count;
        out_result->audio_streams = (uint32_t)audio_tracks.count;

        CMTime duration = asset.duration;
        if (CMTIME_IS_NUMERIC(duration) && duration.timescale > 0) {
            Float64 seconds = CMTimeGetSeconds(duration);
            if (isfinite(seconds) && seconds >= 0) {
                out_result->has_duration = 1;
                out_result->duration_ms = (uint64_t)llround(seconds * 1000.0);
            }
        }

        double bit_rate = 0.0;
        for (AVAssetTrack *track in asset.tracks) {
            bit_rate += track.estimatedDataRate;
        }
        if (bit_rate > 0.0) {
            out_result->has_bit_rate = 1;
            out_result->bit_rate = (uint64_t)llround(bit_rate);
        }

        AVAssetTrack *video_track = video_tracks.firstObject;
        if (video_track != nil) {
            out_result->video.present = 1;
            out_result->video.width = (uint32_t)llround(fabs(video_track.naturalSize.width));
            out_result->video.height = (uint32_t)llround(fabs(video_track.naturalSize.height));
            out_result->video.frame_rate = video_track.nominalFrameRate;

            CMFormatDescriptionRef format =
                (__bridge CMFormatDescriptionRef)video_track.formatDescriptions.firstObject;
            if (format != NULL) {
                player_copy_nsstring(
                    player_video_codec_name(CMFormatDescriptionGetMediaSubType(format)),
                    out_result->video.codec,
                    sizeof(out_result->video.codec));
            }
        }

        AVAssetTrack *audio_track = audio_tracks.firstObject;
        if (audio_track != nil) {
            out_result->audio.present = 1;

            CMAudioFormatDescriptionRef format =
                (__bridge CMAudioFormatDescriptionRef)audio_track.formatDescriptions.firstObject;
            if (format != NULL) {
                const AudioStreamBasicDescription *stream =
                    CMAudioFormatDescriptionGetStreamBasicDescription(format);
                if (stream != NULL) {
                    out_result->audio.sample_rate = (uint32_t)llround(stream->mSampleRate);
                    out_result->audio.channels = (uint16_t)stream->mChannelsPerFrame;
                    player_copy_nsstring(player_audio_codec_name(stream->mFormatID),
                                         out_result->audio.codec,
                                         sizeof(out_result->audio.codec));
                }
            }
        }

        out_result->success = 1;
        return true;
    }
}

typedef struct {
    uint32_t kind;
    uintptr_t handle;
} PlayerMacosVideoSurfaceTarget;

typedef struct {
    uint32_t item_status;
    uint32_t time_control_status;
    float playback_rate;
    uint64_t position_ms;
    uint8_t has_duration;
    uint64_t duration_ms;
    uint8_t reached_end;
    char error_message[256];
} PlayerMacosAvFoundationSnapshot;

typedef void (*PlayerMacosSnapshotCallback)(void *context,
                                            PlayerMacosAvFoundationSnapshot snapshot);
typedef void (*PlayerMacosFirstFrameReadyCallback)(void *context, uint64_t position_ms);
typedef void (*PlayerMacosInterruptionChangedCallback)(void *context, uint8_t interrupted);
typedef void (*PlayerMacosSeekCompletedCallback)(void *context, uint64_t position_ms);
typedef void (*PlayerMacosErrorCallback)(void *context, const char *message);

typedef struct {
    PlayerMacosSnapshotCallback on_snapshot;
    PlayerMacosFirstFrameReadyCallback on_first_frame_ready;
    PlayerMacosInterruptionChangedCallback on_interruption_changed;
    PlayerMacosSeekCompletedCallback on_seek_completed;
    PlayerMacosErrorCallback on_error;
    void *context;
} PlayerMacosAvFoundationCallbacks;

typedef NS_ENUM(uint32_t, PlayerMacosVideoSurfaceKind) {
    PlayerMacosVideoSurfaceKindNsView = 0,
    PlayerMacosVideoSurfaceKindUiView = 1,
    PlayerMacosVideoSurfaceKindPlayerLayer = 2,
    PlayerMacosVideoSurfaceKindMetalLayer = 3,
};

typedef NS_ENUM(uint32_t, PlayerMacosPlayerItemStatusCode) {
    PlayerMacosPlayerItemStatusCodeUnknown = 0,
    PlayerMacosPlayerItemStatusCodeReadyToPlay = 1,
    PlayerMacosPlayerItemStatusCodeFailed = 2,
};

typedef NS_ENUM(uint32_t, PlayerMacosTimeControlStatusCode) {
    PlayerMacosTimeControlStatusCodePaused = 0,
    PlayerMacosTimeControlStatusCodeWaitingToPlay = 1,
    PlayerMacosTimeControlStatusCodePlaying = 2,
};

static void player_write_error_message(NSString *message, char *target, size_t target_size) {
    if (message == nil) {
        player_copy_utf8(NULL, target, target_size);
        return;
    }

    player_copy_nsstring(message, target, target_size);
}

static void player_run_sync_on_main(dispatch_block_t block) {
    if ([NSThread isMainThread]) {
        block();
    } else {
        dispatch_sync(dispatch_get_main_queue(), block);
    }
}

static BOOL player_surface_requires_main_thread(uint32_t surface_kind) {
    return surface_kind == PlayerMacosVideoSurfaceKindNsView ||
           surface_kind == PlayerMacosVideoSurfaceKindMetalLayer;
}

static uint64_t player_time_to_millis(CMTime time) {
    if (!CMTIME_IS_NUMERIC(time) || time.timescale <= 0) {
        return 0;
    }

    Float64 seconds = CMTimeGetSeconds(time);
    if (!isfinite(seconds) || seconds < 0) {
        return 0;
    }

    return (uint64_t)llround(seconds * 1000.0);
}

@interface PlayerMacosAvFoundationSession : NSObject

@property(nonatomic, strong) AVPlayer *player;
@property(nonatomic, strong) AVPlayerItem *playerItem;
@property(nonatomic, strong) AVPlayerLayer *playerLayer;
@property(nonatomic, strong) CALayer *layerHost;
@property(nonatomic, assign) BOOL ownsPlayerLayer;
@property(nonatomic, assign) BOOL observersInstalled;
@property(nonatomic, assign) BOOL requiresMainThread;
@property(nonatomic, strong) id timeObserverToken;
@property(nonatomic, strong) id endObserverToken;
@property(nonatomic, strong) id workspaceSleepObserverToken;
@property(nonatomic, strong) id workspaceWakeObserverToken;
@property(nonatomic, assign) PlayerMacosAvFoundationCallbacks callbacks;
@property(nonatomic, assign) float playbackRate;
@property(nonatomic, assign) BOOL reachedEnd;
@property(nonatomic, assign) BOOL firstFrameReadyReported;
@property(nonatomic, assign) BOOL interrupted;
@property(nonatomic, assign) BOOL resumeAfterInterruption;

- (instancetype)initWithURL:(NSURL *)url
                    surface:(PlayerMacosVideoSurfaceTarget)surface
                  callbacks:(PlayerMacosAvFoundationCallbacks)callbacks
                      error:(NSString **)error;
- (BOOL)replaceSurface:(PlayerMacosVideoSurfaceTarget)surface error:(NSString **)error;
- (BOOL)playWithError:(NSString **)error;
- (BOOL)pauseWithError:(NSString **)error;
- (BOOL)seekToPositionMilliseconds:(uint64_t)position_ms error:(NSString **)error;
- (BOOL)setPlaybackRate:(float)rate error:(NSString **)error;
- (BOOL)stopWithError:(NSString **)error;
- (void)handleWorkspaceWillSleep;
- (void)handleWorkspaceDidWake;

@end

@implementation PlayerMacosAvFoundationSession

- (instancetype)initWithURL:(NSURL *)url
                    surface:(PlayerMacosVideoSurfaceTarget)surface
                  callbacks:(PlayerMacosAvFoundationCallbacks)callbacks
                      error:(NSString **)error {
    self = [super init];
    if (self == nil) {
        return nil;
    }

    AVURLAsset *asset =
        [AVURLAsset URLAssetWithURL:url
                            options:@{
                                AVURLAssetPreferPreciseDurationAndTimingKey : @YES,
                            }];
    self.playerItem = [AVPlayerItem playerItemWithAsset:asset];
    self.player = [AVPlayer playerWithPlayerItem:self.playerItem];
    self.player.actionAtItemEnd = AVPlayerActionAtItemEndPause;
    self.callbacks = callbacks;
    self.playbackRate = 1.0f;
    self.reachedEnd = NO;
    self.firstFrameReadyReported = NO;
    self.interrupted = NO;
    self.resumeAfterInterruption = NO;

    if (![self attachSurface:surface error:error]) {
        return nil;
    }

    [self installObservers];
    [self emitSnapshotWithErrorMessage:nil];
    return self;
}

- (void)dealloc {
    [self uninstallObservers];
    [self detachCurrentSurface];
    self.player = nil;
    self.playerItem = nil;
    PlayerMacosAvFoundationCallbacks callbacks = self.callbacks;
    callbacks.context = NULL;
    callbacks.on_snapshot = NULL;
    callbacks.on_first_frame_ready = NULL;
    callbacks.on_interruption_changed = NULL;
    callbacks.on_seek_completed = NULL;
    callbacks.on_error = NULL;
    self.callbacks = callbacks;
}

- (void)detachCurrentSurface {
    if (self.ownsPlayerLayer) {
        [self.playerLayer removeFromSuperlayer];
    } else if (self.playerLayer != nil) {
        self.playerLayer.player = nil;
    }

    self.playerLayer = nil;
    self.layerHost = nil;
    self.ownsPlayerLayer = NO;
    self.requiresMainThread = NO;
}

- (BOOL)attachSurface:(PlayerMacosVideoSurfaceTarget)surface error:(NSString **)error {
    if (surface.handle == 0) {
        self.requiresMainThread = NO;
        return YES;
    }

    switch ((PlayerMacosVideoSurfaceKind)surface.kind) {
        case PlayerMacosVideoSurfaceKindNsView: {
            NSView *view = (__bridge NSView *)((void *)surface.handle);
            if (view == nil) {
                if (error != NULL) {
                    *error = @"received a null NSView handle for macOS native playback";
                }
                return NO;
            }

            [view setWantsLayer:YES];
            AVPlayerLayer *layer = [AVPlayerLayer playerLayerWithPlayer:self.player];
            layer.frame = view.bounds;
            layer.autoresizingMask = kCALayerWidthSizable | kCALayerHeightSizable;
            [view.layer addSublayer:layer];
            self.playerLayer = layer;
            self.layerHost = view.layer;
            self.ownsPlayerLayer = YES;
            self.requiresMainThread = YES;
            return YES;
        }
        case PlayerMacosVideoSurfaceKindPlayerLayer: {
            AVPlayerLayer *layer = (__bridge AVPlayerLayer *)((void *)surface.handle);
            if (layer == nil) {
                if (error != NULL) {
                    *error = @"received a null AVPlayerLayer handle for macOS native playback";
                }
                return NO;
            }

            layer.player = self.player;
            self.playerLayer = layer;
            self.layerHost = layer.superlayer;
            self.ownsPlayerLayer = NO;
            self.requiresMainThread = NO;
            return YES;
        }
        case PlayerMacosVideoSurfaceKindMetalLayer: {
            CALayer *layer_host = (__bridge CALayer *)((void *)surface.handle);
            if (layer_host == nil) {
                if (error != NULL) {
                    *error = @"received a null CALayer handle for macOS native playback";
                }
                return NO;
            }

            AVPlayerLayer *layer = [AVPlayerLayer playerLayerWithPlayer:self.player];
            layer.frame = layer_host.bounds;
            layer.autoresizingMask = kCALayerWidthSizable | kCALayerHeightSizable;
            [layer_host addSublayer:layer];
            self.playerLayer = layer;
            self.layerHost = layer_host;
            self.ownsPlayerLayer = YES;
            self.requiresMainThread = YES;
            return YES;
        }
        case PlayerMacosVideoSurfaceKindUiView:
            if (error != NULL) {
                *error = @"UiView is not a valid macOS video surface";
            }
            return NO;
    }

    if (error != NULL) {
        *error = @"unsupported macOS native video surface kind";
    }
    return NO;
}

- (void)installPlayerLayerObserverIfNeeded {
    if (!self.observersInstalled || self.playerLayer == nil) {
        return;
    }

    [self.playerLayer addObserver:self
                       forKeyPath:@"readyForDisplay"
                          options:NSKeyValueObservingOptionInitial | NSKeyValueObservingOptionNew
                          context:NULL];
}

- (void)uninstallPlayerLayerObserverIfNeeded {
    if (self.playerLayer == nil) {
        return;
    }

    @try {
        [self.playerLayer removeObserver:self forKeyPath:@"readyForDisplay"];
    } @catch (__unused NSException *exception) {
    }
}

- (BOOL)replaceSurface:(PlayerMacosVideoSurfaceTarget)surface error:(NSString **)error {
    [self uninstallPlayerLayerObserverIfNeeded];
    [self detachCurrentSurface];

    if (![self attachSurface:surface error:error]) {
        return NO;
    }

    self.firstFrameReadyReported = NO;
    [self installPlayerLayerObserverIfNeeded];
    [self reportFirstFrameReadyIfNeeded];
    [self emitSnapshotWithErrorMessage:nil];
    return YES;
}

- (void)installObservers {
    if (self.observersInstalled) {
        return;
    }

    [self.player addObserver:self
                  forKeyPath:@"timeControlStatus"
                     options:NSKeyValueObservingOptionInitial | NSKeyValueObservingOptionNew
                     context:NULL];
    [self installPlayerLayerObserverIfNeeded];
    [self.playerItem addObserver:self
                      forKeyPath:@"status"
                         options:NSKeyValueObservingOptionInitial | NSKeyValueObservingOptionNew
                         context:NULL];

    __weak typeof(self) weak_self = self;
    self.timeObserverToken =
        [self.player addPeriodicTimeObserverForInterval:CMTimeMakeWithSeconds(0.1, 1000)
                                                  queue:dispatch_get_main_queue()
                                             usingBlock:^(CMTime time) {
                                               typeof(weak_self) strong_self = weak_self;
                                               if (strong_self == nil) {
                                                   return;
                                               }
                                               [strong_self emitSnapshotWithErrorMessage:nil];
                                             }];
    self.endObserverToken = [[NSNotificationCenter defaultCenter]
        addObserverForName:AVPlayerItemDidPlayToEndTimeNotification
                    object:self.playerItem
                     queue:[NSOperationQueue mainQueue]
                usingBlock:^(NSNotification *note) {
                  typeof(weak_self) strong_self = weak_self;
                  if (strong_self == nil) {
                      return;
                  }
                  strong_self.reachedEnd = YES;
                  [strong_self emitSnapshotWithErrorMessage:nil];
                }];
    NSNotificationCenter *workspace_notification_center =
        [[NSWorkspace sharedWorkspace] notificationCenter];
    self.workspaceSleepObserverToken =
        [workspace_notification_center addObserverForName:NSWorkspaceWillSleepNotification
                                                   object:nil
                                                    queue:[NSOperationQueue mainQueue]
                                               usingBlock:^(__unused NSNotification *note) {
                                                 typeof(weak_self) strong_self = weak_self;
                                                 if (strong_self == nil) {
                                                     return;
                                                 }
                                                 [strong_self handleWorkspaceWillSleep];
                                               }];
    self.workspaceWakeObserverToken =
        [workspace_notification_center addObserverForName:NSWorkspaceDidWakeNotification
                                                   object:nil
                                                    queue:[NSOperationQueue mainQueue]
                                               usingBlock:^(__unused NSNotification *note) {
                                                 typeof(weak_self) strong_self = weak_self;
                                                 if (strong_self == nil) {
                                                     return;
                                                 }
                                                 [strong_self handleWorkspaceDidWake];
                                               }];

    self.observersInstalled = YES;
}

- (void)uninstallObservers {
    if (!self.observersInstalled) {
        return;
    }

    @try {
        [self.player removeObserver:self forKeyPath:@"timeControlStatus"];
    } @catch (__unused NSException *exception) {
    }
    [self uninstallPlayerLayerObserverIfNeeded];
    @try {
        [self.playerItem removeObserver:self forKeyPath:@"status"];
    } @catch (__unused NSException *exception) {
    }

    if (self.timeObserverToken != nil) {
        [self.player removeTimeObserver:self.timeObserverToken];
        self.timeObserverToken = nil;
    }
    if (self.endObserverToken != nil) {
        [[NSNotificationCenter defaultCenter] removeObserver:self.endObserverToken];
        self.endObserverToken = nil;
    }
    if (self.workspaceSleepObserverToken != nil) {
        [[[NSWorkspace sharedWorkspace] notificationCenter]
            removeObserver:self.workspaceSleepObserverToken];
        self.workspaceSleepObserverToken = nil;
    }
    if (self.workspaceWakeObserverToken != nil) {
        [[[NSWorkspace sharedWorkspace] notificationCenter]
            removeObserver:self.workspaceWakeObserverToken];
        self.workspaceWakeObserverToken = nil;
    }

    self.observersInstalled = NO;
}

- (PlayerMacosPlayerItemStatusCode)itemStatusCode {
    switch (self.playerItem.status) {
        case AVPlayerItemStatusReadyToPlay:
            return PlayerMacosPlayerItemStatusCodeReadyToPlay;
        case AVPlayerItemStatusFailed:
            return PlayerMacosPlayerItemStatusCodeFailed;
        case AVPlayerItemStatusUnknown:
        default:
            return PlayerMacosPlayerItemStatusCodeUnknown;
    }
}

- (PlayerMacosTimeControlStatusCode)timeControlStatusCode {
    switch (self.player.timeControlStatus) {
        case AVPlayerTimeControlStatusPlaying:
            return PlayerMacosTimeControlStatusCodePlaying;
        case AVPlayerTimeControlStatusWaitingToPlayAtSpecifiedRate:
            return PlayerMacosTimeControlStatusCodeWaitingToPlay;
        case AVPlayerTimeControlStatusPaused:
        default:
            return PlayerMacosTimeControlStatusCodePaused;
    }
}

- (PlayerMacosAvFoundationSnapshot)currentSnapshotWithErrorMessage:(NSString *)error_message {
    PlayerMacosAvFoundationSnapshot snapshot;
    memset(&snapshot, 0, sizeof(snapshot));
    snapshot.item_status = [self itemStatusCode];
    snapshot.time_control_status = [self timeControlStatusCode];
    snapshot.playback_rate = self.playbackRate > 0.0f ? self.playbackRate : 1.0f;
    snapshot.position_ms = player_time_to_millis(self.player.currentTime);
    snapshot.reached_end = self.reachedEnd ? 1 : 0;

    CMTime duration = self.playerItem.duration;
    if (CMTIME_IS_NUMERIC(duration) && duration.timescale > 0) {
        snapshot.has_duration = 1;
        snapshot.duration_ms = player_time_to_millis(duration);
    }

    NSString *resolved_error = error_message;
    if (resolved_error == nil && self.playerItem.status == AVPlayerItemStatusFailed) {
        resolved_error = self.playerItem.error.localizedDescription;
    }
    player_write_error_message(resolved_error, snapshot.error_message, sizeof(snapshot.error_message));

    return snapshot;
}

- (void)emitSnapshotWithErrorMessage:(NSString *)error_message {
    if (self.callbacks.context == NULL || self.callbacks.on_snapshot == NULL) {
        return;
    }

    self.callbacks.on_snapshot(self.callbacks.context,
                               [self currentSnapshotWithErrorMessage:error_message]);
}

- (void)emitInterruptionChanged:(BOOL)interrupted {
    if (self.callbacks.context == NULL || self.callbacks.on_interruption_changed == NULL) {
        return;
    }

    self.callbacks.on_interruption_changed(self.callbacks.context, interrupted ? 1 : 0);
}

- (void)emitErrorMessage:(NSString *)error_message {
    if (error_message == nil || self.callbacks.context == NULL ||
        self.callbacks.on_error == NULL) {
        return;
    }

    self.callbacks.on_error(self.callbacks.context, error_message.UTF8String);
}

- (void)reportSeekCompletedAtPosition:(CMTime)position {
    if (self.callbacks.context == NULL || self.callbacks.on_seek_completed == NULL) {
        return;
    }

    self.callbacks.on_seek_completed(self.callbacks.context, player_time_to_millis(position));
}

- (void)reportFirstFrameReadyIfNeeded {
    if (self.firstFrameReadyReported || self.playerLayer == nil || !self.playerLayer.readyForDisplay) {
        return;
    }
    if (self.callbacks.context == NULL || self.callbacks.on_first_frame_ready == NULL) {
        return;
    }

    self.firstFrameReadyReported = YES;
    self.callbacks.on_first_frame_ready(self.callbacks.context,
                                        player_time_to_millis(self.player.currentTime));
}

- (void)handleWorkspaceWillSleep {
    BOOL was_playing =
        self.player.timeControlStatus == AVPlayerTimeControlStatusPlaying || self.player.rate > 0.0f;
    self.resumeAfterInterruption = was_playing;
    if (self.interrupted) {
        return;
    }

    self.interrupted = YES;
    if (was_playing) {
        [self.player pause];
    }
    [self emitInterruptionChanged:YES];
    [self emitSnapshotWithErrorMessage:nil];
}

- (void)handleWorkspaceDidWake {
    BOOL should_resume = self.resumeAfterInterruption;
    self.resumeAfterInterruption = NO;
    if (!self.interrupted) {
        if (should_resume) {
            NSString *resume_error = nil;
            if (![self playWithError:&resume_error]) {
                [self emitErrorMessage:resume_error];
                [self emitSnapshotWithErrorMessage:resume_error];
            }
        }
        return;
    }

    self.interrupted = NO;
    [self emitInterruptionChanged:NO];
    if (should_resume) {
        NSString *resume_error = nil;
        if (![self playWithError:&resume_error]) {
            [self emitErrorMessage:resume_error];
            [self emitSnapshotWithErrorMessage:resume_error];
        }
        return;
    }

    [self emitSnapshotWithErrorMessage:nil];
}

- (BOOL)playWithError:(NSString **)error {
    if (self.playerItem.status == AVPlayerItemStatusFailed) {
        if (error != NULL) {
            *error = self.playerItem.error.localizedDescription ?: @"AVPlayerItem failed";
        }
        return NO;
    }

    self.reachedEnd = NO;
    self.resumeAfterInterruption = YES;
    float requested_rate = self.playbackRate > 0.0f ? self.playbackRate : 1.0f;
    if (@available(macOS 10.12, *)) {
        [self.player playImmediatelyAtRate:requested_rate];
    } else {
        self.player.rate = requested_rate;
        [self.player play];
    }
    [self emitSnapshotWithErrorMessage:nil];
    return YES;
}

- (BOOL)pauseWithError:(NSString **)error {
    (void)error;
    self.resumeAfterInterruption = NO;
    [self.player pause];
    [self emitSnapshotWithErrorMessage:nil];
    return YES;
}

- (BOOL)seekToPositionMilliseconds:(uint64_t)position_ms error:(NSString **)error {
    (void)error;
    self.reachedEnd = NO;
    CMTime position = CMTimeMake((int64_t)position_ms, 1000);
    __weak typeof(self) weak_self = self;
    [self.player seekToTime:position
          toleranceBefore:kCMTimeZero
           toleranceAfter:kCMTimeZero
        completionHandler:^(BOOL finished) {
          typeof(weak_self) strong_self = weak_self;
          if (strong_self == nil) {
              return;
          }
          if (finished) {
              [strong_self reportSeekCompletedAtPosition:position];
              [strong_self emitSnapshotWithErrorMessage:nil];
          } else {
              NSString *message = @"AVPlayer seek did not finish successfully";
              [strong_self emitErrorMessage:message];
              [strong_self emitSnapshotWithErrorMessage:message];
          }
        }];
    return YES;
}

- (BOOL)setPlaybackRate:(float)rate error:(NSString **)error {
    (void)error;
    self.playbackRate = rate > 0.0f ? rate : 1.0f;
    if (self.player.timeControlStatus == AVPlayerTimeControlStatusPlaying || self.player.rate > 0.0f) {
        self.player.rate = self.playbackRate;
    }
    [self emitSnapshotWithErrorMessage:nil];
    return YES;
}

- (BOOL)stopWithError:(NSString **)error {
    (void)error;
    self.reachedEnd = NO;
    self.resumeAfterInterruption = NO;
    [self.player pause];
    return [self seekToPositionMilliseconds:0 error:error];
}

- (void)observeValueForKeyPath:(NSString *)key_path
                      ofObject:(id)object
                        change:(NSDictionary<NSKeyValueChangeKey, id> *)change
                       context:(void *)context {
    (void)object;
    (void)change;
    (void)context;

    if ([key_path isEqualToString:@"status"] && self.playerItem.status == AVPlayerItemStatusFailed) {
        NSString *message = self.playerItem.error.localizedDescription ?: @"AVPlayerItem failed";
        [self emitErrorMessage:message];
        [self emitSnapshotWithErrorMessage:message];
        return;
    }

    if ([key_path isEqualToString:@"readyForDisplay"]) {
        [self reportFirstFrameReadyIfNeeded];
    }

    [self emitSnapshotWithErrorMessage:nil];
}

@end

bool player_macos_avfoundation_create_session(const char *source,
                                              PlayerMacosVideoSurfaceTarget surface,
                                              PlayerMacosAvFoundationCallbacks callbacks,
                                              void **out_session,
                                              char *error_message,
                                              size_t error_message_size) {
    if (out_session == NULL) {
        player_copy_utf8("output session pointer must not be null",
                         error_message,
                         error_message_size);
        return false;
    }

    @autoreleasepool {
        __block PlayerMacosAvFoundationSession *session = nil;
        __block NSString *create_error = nil;
        void (^create_block)(void) = ^{
          NSURL *url = player_source_url_from_utf8(source);
          if (url == nil) {
              create_error = @"failed to create NSURL from media source";
              return;
          }

          session = [[PlayerMacosAvFoundationSession alloc] initWithURL:url
                                                                surface:surface
                                                              callbacks:callbacks
                                                                  error:&create_error];
        };
        if (surface.kind == PlayerMacosVideoSurfaceKindNsView ||
            surface.kind == PlayerMacosVideoSurfaceKindMetalLayer) {
            player_run_sync_on_main(create_block);
        } else {
            create_block();
        }

        if (session == nil) {
            player_write_error_message(create_error, error_message, error_message_size);
            return false;
        }

        *out_session = (__bridge_retained void *)session;
        player_copy_utf8(NULL, error_message, error_message_size);
        return true;
    }
}

void player_macos_avfoundation_destroy_session(void *session_handle) {
    if (session_handle == NULL) {
        return;
    }

    PlayerMacosAvFoundationSession *session =
        (__bridge PlayerMacosAvFoundationSession *)session_handle;
    void (^destroy_block)(void) = ^{
      PlayerMacosAvFoundationSession *session =
          (__bridge_transfer PlayerMacosAvFoundationSession *)session_handle;
      (void)session;
    };
    if (session.requiresMainThread) {
        player_run_sync_on_main(destroy_block);
    } else {
        destroy_block();
    }
}

static bool player_macos_perform_command(void *session_handle,
                                         char *error_message,
                                         size_t error_message_size,
                                         BOOL (^command)(PlayerMacosAvFoundationSession *session,
                                                         NSString **error)) {
    if (session_handle == NULL) {
        player_copy_utf8("AVFoundation session handle must not be null",
                         error_message,
                         error_message_size);
        return false;
    }

    PlayerMacosAvFoundationSession *session =
        (__bridge PlayerMacosAvFoundationSession *)session_handle;
    __block BOOL succeeded = NO;
    __block NSString *command_error = nil;
    void (^command_block)(void) = ^{
      PlayerMacosAvFoundationSession *session =
          (__bridge PlayerMacosAvFoundationSession *)session_handle;
      succeeded = command(session, &command_error);
    };
    if (session.requiresMainThread) {
        player_run_sync_on_main(command_block);
    } else {
        command_block();
    }
    if (!succeeded) {
        player_write_error_message(command_error, error_message, error_message_size);
        return false;
    }

    player_copy_utf8(NULL, error_message, error_message_size);
    return true;
}

static bool player_macos_perform_surface_command(
    void *session_handle,
    BOOL run_on_main,
    char *error_message,
    size_t error_message_size,
    BOOL (^command)(PlayerMacosAvFoundationSession *session, NSString **error)) {
    if (session_handle == NULL) {
        player_copy_utf8("AVFoundation session handle must not be null",
                         error_message,
                         error_message_size);
        return false;
    }

    __block BOOL succeeded = NO;
    __block NSString *command_error = nil;
    void (^command_block)(void) = ^{
      PlayerMacosAvFoundationSession *session =
          (__bridge PlayerMacosAvFoundationSession *)session_handle;
      succeeded = command(session, &command_error);
    };

    if (run_on_main) {
        player_run_sync_on_main(command_block);
    } else {
        command_block();
    }

    if (!succeeded) {
        player_write_error_message(command_error, error_message, error_message_size);
        return false;
    }

    player_copy_utf8(NULL, error_message, error_message_size);
    return true;
}

bool player_macos_avfoundation_session_play(void *session_handle,
                                            char *error_message,
                                            size_t error_message_size) {
    return player_macos_perform_command(
        session_handle,
        error_message,
        error_message_size,
        ^BOOL(PlayerMacosAvFoundationSession *session, NSString **error) {
          return [session playWithError:error];
        });
}

bool player_macos_avfoundation_session_pause(void *session_handle,
                                             char *error_message,
                                             size_t error_message_size) {
    return player_macos_perform_command(
        session_handle,
        error_message,
        error_message_size,
        ^BOOL(PlayerMacosAvFoundationSession *session, NSString **error) {
          return [session pauseWithError:error];
        });
}

bool player_macos_avfoundation_session_seek_to(void *session_handle,
                                               uint64_t position_ms,
                                               char *error_message,
                                               size_t error_message_size) {
    return player_macos_perform_command(
        session_handle,
        error_message,
        error_message_size,
        ^BOOL(PlayerMacosAvFoundationSession *session, NSString **error) {
          return [session seekToPositionMilliseconds:position_ms error:error];
        });
}

bool player_macos_avfoundation_session_set_playback_rate(void *session_handle,
                                                         float rate,
                                                         char *error_message,
                                                         size_t error_message_size) {
    return player_macos_perform_command(
        session_handle,
        error_message,
        error_message_size,
        ^BOOL(PlayerMacosAvFoundationSession *session, NSString **error) {
          return [session setPlaybackRate:rate error:error];
        });
}

bool player_macos_avfoundation_session_stop(void *session_handle,
                                            char *error_message,
                                            size_t error_message_size) {
    return player_macos_perform_command(
        session_handle,
        error_message,
        error_message_size,
        ^BOOL(PlayerMacosAvFoundationSession *session, NSString **error) {
          return [session stopWithError:error];
        });
}

bool player_macos_avfoundation_session_attach_surface(void *session_handle,
                                                      PlayerMacosVideoSurfaceTarget surface,
                                                      char *error_message,
                                                      size_t error_message_size) {
    PlayerMacosAvFoundationSession *session =
        (__bridge PlayerMacosAvFoundationSession *)session_handle;
    BOOL run_on_main =
        session != nil && (session.requiresMainThread || player_surface_requires_main_thread(surface.kind));
    return player_macos_perform_surface_command(
        session_handle,
        run_on_main,
        error_message,
        error_message_size,
        ^BOOL(PlayerMacosAvFoundationSession *session, NSString **error) {
          return [session replaceSurface:surface error:error];
        });
}

bool player_macos_avfoundation_session_detach_surface(void *session_handle,
                                                      char *error_message,
                                                      size_t error_message_size) {
    PlayerMacosVideoSurfaceTarget detached_surface;
    memset(&detached_surface, 0, sizeof(detached_surface));
    PlayerMacosAvFoundationSession *session =
        (__bridge PlayerMacosAvFoundationSession *)session_handle;
    BOOL run_on_main = session != nil && session.requiresMainThread;
    return player_macos_perform_surface_command(
        session_handle,
        run_on_main,
        error_message,
        error_message_size,
        ^BOOL(PlayerMacosAvFoundationSession *session, NSString **error) {
          return [session replaceSurface:detached_surface error:error];
        });
}

void *player_macos_test_create_player_layer(void) {
    @autoreleasepool {
        AVPlayerLayer *layer = [AVPlayerLayer playerLayerWithPlayer:nil];
        return (__bridge_retained void *)layer;
    }
}

void player_macos_test_release_object(void *handle) {
    if (handle == NULL) {
        return;
    }

    @autoreleasepool {
        id object = (__bridge_transfer id)handle;
        (void)object;
    }
}
