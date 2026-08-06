import Foundation
internal import VesperPlayerKitBridgeShim

extension VesperDownloadManager {
    func syncRuntimeState(processCommands: Bool) {
        guard sessionHandle != 0 else {
            taskStore.replaceAll(VesperDownloadSnapshot(tasks: []))
            snapshot = VesperDownloadSnapshot(tasks: [])
            eventBuffer.removeAll(keepingCapacity: false)
            droppedBufferedEvents = 0
            pendingSnapshotResync = false
            lastProgressPersistence.removeAll(keepingCapacity: false)
            needsAuthoritativeSnapshotResync = false
            isProcessingRuntimeCommands = false
            pendingRuntimeCommandAcknowledgementCount = 0
            runtimeCommandDiagnostic = nil
            return
        }

        var runtimeEvents = VesperRuntimeDownloadEventList(
            events: nil,
            len: 0,
            dropped_events: 0
        )
        var events: [VesperDownloadEvent] = []
        if bindings.drainDownloadEvents(sessionHandle: sessionHandle, outEvents: &runtimeEvents) {
            let drainedEventCount = Int(runtimeEvents.len)
            if runtimeEvents.dropped_events > 0 {
                needsAuthoritativeSnapshotResync = true
                recordDroppedEvents(
                    nativeDroppedEvents: runtimeEvents.dropped_events,
                    discardedEvents: drainedEventCount
                )
            } else if let decodedEvents = runtimeEvents.decodePublicEvents() {
                if !needsAuthoritativeSnapshotResync {
                    events = decodedEvents
                    appendEventsCapped(events)
                } else {
                    recordDroppedEvents(
                        nativeDroppedEvents: 0,
                        discardedEvents: drainedEventCount
                    )
                }
            } else {
                needsAuthoritativeSnapshotResync = true
                recordDroppedEvents(
                    nativeDroppedEvents: 0,
                    discardedEvents: drainedEventCount
                )
            }
            bindings.freeDownloadEventList(&runtimeEvents)
        } else {
            needsAuthoritativeSnapshotResync = true
            markSnapshotResyncRequired()
        }

        let immediateEvents = events.filter { !$0.isRemovedStatePatch }
        if !immediateEvents.isEmpty {
            let updatedSnapshot = taskStore.apply(immediateEvents)
            if updatedSnapshot != snapshot {
                snapshot = updatedSnapshot
            }
        }

        if processCommands {
            processRuntimeCommandsIfNeeded()
        }

        if needsAuthoritativeSnapshotResync,
           replaceTaskStoreWithRuntimeSnapshot()
        {
            needsAuthoritativeSnapshotResync = false
        }

        if !events.isEmpty {
            let removalEvents = events.filter(\.isRemovedStatePatch)
            if !removalEvents.isEmpty {
                let updatedSnapshot = taskStore.apply(removalEvents)
                if updatedSnapshot != snapshot {
                    snapshot = updatedSnapshot
                }
            }
            if shouldPersistSnapshot(after: events) {
                persistSnapshot(snapshot)
            }
        }
    }

    func forceFullSync(processCommands: Bool) {
        guard sessionHandle != 0 else {
            taskStore.replaceAll(VesperDownloadSnapshot(tasks: []))
            snapshot = VesperDownloadSnapshot(tasks: [])
            eventBuffer.removeAll(keepingCapacity: false)
            droppedBufferedEvents = 0
            pendingSnapshotResync = false
            lastProgressPersistence.removeAll(keepingCapacity: false)
            needsAuthoritativeSnapshotResync = false
            isProcessingRuntimeCommands = false
            pendingRuntimeCommandAcknowledgementCount = 0
            runtimeCommandDiagnostic = nil
            return
        }

        needsAuthoritativeSnapshotResync = !replaceTaskStoreWithRuntimeSnapshot()
        if needsAuthoritativeSnapshotResync {
            markSnapshotResyncRequired()
        }

        syncRuntimeState(processCommands: processCommands)
    }

    func processRuntimeCommandsIfNeeded() {
        guard !isProcessingRuntimeCommands else {
            return
        }
        if runtimeCommandDiagnostic != nil {
            _ = acknowledgePendingRuntimeCommandsIfNeeded()
            return
        }
        isProcessingRuntimeCommands = true
        defer { isProcessingRuntimeCommands = false }

        for _ in 0..<maxRuntimeCommandBatchesPerSync {
            guard acknowledgePendingRuntimeCommandsIfNeeded() else {
                return
            }

            var runtimeCommands = VesperRuntimeDownloadCommandList(commands: nil, len: 0)
            guard bindings.peekDownloadCommands(
                sessionHandle: sessionHandle,
                outCommands: &runtimeCommands
            ) else {
                return
            }
            let commandCount = runtimeCommands.len
            let commands = runtimeCommands.decodePublicCommands()
            bindings.freeDownloadCommandList(&runtimeCommands)
            guard let commands else {
                let diagnostic =
                    "Native download command validation failed; command processing is quarantined."
                runtimeCommandDiagnostic = diagnostic
                iosHostLog(diagnostic)
                pendingRuntimeCommandAcknowledgementCount = commandCount
                _ = acknowledgePendingRuntimeCommandsIfNeeded()
                return
            }

            guard commandCount > 0 else {
                return
            }
            commands.forEach(applyCommand(_:))
            pendingRuntimeCommandAcknowledgementCount = commandCount
            guard acknowledgePendingRuntimeCommandsIfNeeded() else {
                return
            }
        }
    }

    func acknowledgePendingRuntimeCommandsIfNeeded() -> Bool {
        guard pendingRuntimeCommandAcknowledgementCount > 0 else {
            return true
        }
        guard bindings.acknowledgeDownloadCommands(
            sessionHandle: sessionHandle,
            commandCount: pendingRuntimeCommandAcknowledgementCount
        ) else {
            return false
        }
        pendingRuntimeCommandAcknowledgementCount = 0
        return true
    }

    @discardableResult
    private func replaceTaskStoreWithRuntimeSnapshot() -> Bool {
        var runtimeSnapshot = VesperRuntimeDownloadSnapshot(tasks: nil, len: 0)
        guard bindings.downloadSessionSnapshot(
            sessionHandle: sessionHandle,
            outSnapshot: &runtimeSnapshot
        ) else {
            return false
        }
        let fullSnapshot = runtimeSnapshot.decodePublic()
        bindings.freeDownloadSnapshot(&runtimeSnapshot)
        guard let fullSnapshot else {
            return false
        }
        taskStore.replaceAll(fullSnapshot)
        let activeSnapshot = taskStore.snapshot()
        snapshot = activeSnapshot
        persistSnapshot(activeSnapshot)
        return true
    }

    func shouldPersistSnapshot(after events: [VesperDownloadEvent]) -> Bool {
        var shouldPersist = false
        for event in events {
            switch event {
            case .created, .assetIndexUpdated:
                shouldPersist = true
            case let .stateChanged(patch):
                shouldPersist = true
                lastProgressPersistence[patch.taskId] = (patch.progress.receivedBytes, Date())
            case let .progressUpdated(patch):
                if shouldPersistProgressCheckpoint(patch) {
                    shouldPersist = true
                }
            }
        }
        return shouldPersist
    }

    func shouldPersistProgressCheckpoint(_ patch: VesperDownloadTaskProgressPatch) -> Bool {
        let now = Date()
        guard let previous = lastProgressPersistence[patch.taskId] else {
            lastProgressPersistence[patch.taskId] = (patch.progress.receivedBytes, now)
            return true
        }
        let byteDelta = patch.progress.receivedBytes >= previous.bytes
            ? patch.progress.receivedBytes - previous.bytes
            : 0
        let elapsedMs = UInt64(max(0, now.timeIntervalSince(previous.date) * 1000))
        guard byteDelta >= configuration.minProgressBytes,
              elapsedMs >= configuration.minProgressIntervalMs
        else {
            return false
        }
        lastProgressPersistence[patch.taskId] = (patch.progress.receivedBytes, now)
        return true
    }

    func applyCommand(_ command: RuntimeDownloadCommand) {
        switch command.kind {
        case .prepare:
            guard let task = command.task else {
                return
            }
            executor.prepare(task: task, reporter: runtimeReporter)
        case .start:
            guard let task = command.task else {
                return
            }
            executor.start(task: task, reporter: runtimeReporter)
        case .resume:
            guard let task = command.task else {
                return
            }
            executor.resume(task: task, reporter: runtimeReporter)
        case .pause:
            executor.pause(taskId: command.taskId)
        case .remove:
            executor.remove(task: command.task)
        }
    }

    var runtimeReporter: any VesperDownloadExecutionReporter {
        RuntimeReporter(manager: self)
    }

    /// Appends a batch of drained download events to `eventBuffer` while
    /// enforcing `maxEventBufferCapacity`.
    ///
    /// This runs on the `@MainActor` (download manager is main-actor-isolated),
    /// so the truncation must not stall the UI. A pathological drain that
    /// returns more events than the buffer can hold is handled by keeping only
    /// the newest `maxEventBufferCapacity` events of the batch via a slice
    /// replacement (O(batch size), no shifting of the existing buffer), instead
    /// of growing the buffer and then shifting it with `removeFirst`.
    func appendEventsCapped(_ events: [VesperDownloadEvent]) {
        let capacity = maxEventBufferCapacity
        if events.count >= capacity {
            // The batch alone fills the buffer; drop its older tail and replace
            // the buffer outright so we never shift a large existing array.
            let discardedCount = eventBuffer.count + events.count - capacity
            eventBuffer = Array(events.suffix(capacity))
            recordDroppedEvents(nativeDroppedEvents: 0, discardedEvents: discardedCount)
            return
        }
        eventBuffer.append(contentsOf: events)
        let excess = eventBuffer.count - capacity
        if excess > 0 {
            eventBuffer.removeFirst(excess)
            recordDroppedEvents(nativeDroppedEvents: 0, discardedEvents: excess)
        }
    }

    func recordDroppedEvents(nativeDroppedEvents: UInt64, discardedEvents: Int) {
        pendingSnapshotResync = true
        droppedBufferedEvents = saturatingAdd(droppedBufferedEvents, nativeDroppedEvents)
        if discardedEvents > 0 {
            droppedBufferedEvents = saturatingAdd(
                droppedBufferedEvents,
                UInt64(discardedEvents)
            )
        }
    }

    func markSnapshotResyncRequired() {
        pendingSnapshotResync = true
    }

    func saturatingAdd(_ lhs: UInt64, _ rhs: UInt64) -> UInt64 {
        let result = lhs.addingReportingOverflow(rhs)
        return result.overflow ? UInt64.max : result.partialValue
    }
}
