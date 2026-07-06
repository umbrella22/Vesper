import Foundation
import VesperPlayerKitBridgeShim

extension VesperDownloadManager {
    func syncRuntimeState(processCommands: Bool) {
        guard sessionHandle != 0 else {
            taskStore.replaceAll(VesperDownloadSnapshot(tasks: []))
            snapshot = VesperDownloadSnapshot(tasks: [])
            eventBuffer.removeAll(keepingCapacity: false)
            lastProgressPersistence.removeAll(keepingCapacity: false)
            return
        }

        var runtimeEvents = VesperRuntimeDownloadEventList(events: nil, len: 0)
        var events: [VesperDownloadEvent] = []
        if bindings.drainDownloadEvents(sessionHandle: sessionHandle, outEvents: &runtimeEvents) {
            events = runtimeEvents.toPublic()
            eventBuffer.append(contentsOf: events)
            if eventBuffer.count > maxEventBufferCapacity {
                eventBuffer.removeFirst(eventBuffer.count - maxEventBufferCapacity)
            }
            bindings.freeDownloadEventList(&runtimeEvents)
        }

        let immediateEvents = events.filter { !$0.isRemovedStatePatch }
        if !immediateEvents.isEmpty {
            let updatedSnapshot = taskStore.apply(immediateEvents)
            if updatedSnapshot != snapshot {
                snapshot = updatedSnapshot
            }
        }

        if processCommands {
            var runtimeCommands = VesperRuntimeDownloadCommandList(commands: nil, len: 0)
            if bindings.drainDownloadCommands(sessionHandle: sessionHandle, outCommands: &runtimeCommands) {
                let commands = runtimeCommands.toPublic()
                bindings.freeDownloadCommandList(&runtimeCommands)
                commands.forEach(applyCommand(_:))
            }
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
            lastProgressPersistence.removeAll(keepingCapacity: false)
            return
        }

        var runtimeSnapshot = VesperRuntimeDownloadSnapshot(tasks: nil, len: 0)
        if bindings.downloadSessionSnapshot(sessionHandle: sessionHandle, outSnapshot: &runtimeSnapshot) {
            let fullSnapshot = runtimeSnapshot.toPublic()
            taskStore.replaceAll(fullSnapshot)
            let activeSnapshot = taskStore.snapshot()
            snapshot = activeSnapshot
            persistSnapshot(activeSnapshot)
            bindings.freeDownloadSnapshot(&runtimeSnapshot)
        } else {
            taskStore.replaceAll(VesperDownloadSnapshot(tasks: []))
            snapshot = VesperDownloadSnapshot(tasks: [])
        }

        syncRuntimeState(processCommands: processCommands)
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
            executor.remove(task: task(command.taskId))
        }
    }

    var runtimeReporter: any VesperDownloadExecutionReporter {
        RuntimeReporter(manager: self)
    }
}
